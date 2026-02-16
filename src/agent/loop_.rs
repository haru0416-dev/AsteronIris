use crate::config::Config;
use crate::memory::{
    self, Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
};
use crate::observability::{self, Observer, ObserverEvent};
use crate::persona::state_header::StateHeaderV1;
use crate::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::writeback_guard::{
    validate_writeback_payload, ImmutableStateHeader, WritebackGuardVerdict,
};
use crate::security::SecurityPolicy;
use crate::tools;
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use serde_json::Value;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

const PERSONA_PER_TURN_CALL_BUDGET: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TurnCallAccounting {
    budget_limit: u8,
    answer_calls: u8,
    reflect_calls: u8,
}

impl TurnCallAccounting {
    fn for_persona_mode(enabled: bool) -> Self {
        Self {
            budget_limit: if enabled {
                PERSONA_PER_TURN_CALL_BUDGET
            } else {
                1
            },
            answer_calls: 0,
            reflect_calls: 0,
        }
    }

    fn total_calls(self) -> u8 {
        self.answer_calls + self.reflect_calls
    }

    fn consume_answer_call(&mut self) -> Result<()> {
        self.answer_calls = self.answer_calls.saturating_add(1);
        self.ensure_budget()
    }

    fn consume_reflect_call(&mut self) -> Result<()> {
        self.reflect_calls = self.reflect_calls.saturating_add(1);
        self.ensure_budget()
    }

    fn ensure_budget(self) -> Result<()> {
        if self.total_calls() > self.budget_limit {
            anyhow::bail!(
                "persona per-turn call budget exceeded: consumed={} budget={}",
                self.total_calls(),
                self.budget_limit
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TurnExecutionOutcome {
    response: String,
    accounting: TurnCallAccounting,
}

struct MainSessionTurnParams<'a> {
    answer_provider: &'a dyn Provider,
    reflect_provider: &'a dyn Provider,
    system_prompt: &'a str,
    model_name: &'a str,
    temperature: f64,
}

/// Build context preamble by searching memory for relevant entries
async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    let mut context = String::new();

    // Pull relevant memories for this message
    let query = RecallQuery {
        entity_id: "default".to_string(),
        query: user_msg.to_string(),
        limit: 5,
    };
    if let Ok(entries) = mem.recall_scoped(query).await {
        if !entries.is_empty() {
            context.push_str("[Memory context]\n");
            for entry in &entries {
                let _ = writeln!(context, "- {}: {}", entry.slot_key, entry.value);
            }
            context.push('\n');
        }
    }

    context
}

const PERSONA_REFLECT_SYSTEM_PROMPT: &str = r#"You are a deterministic reflection/writeback stage.
Output must be a single strict JSON object, with no markdown and no extra text.

Required top-level shape:
{
  "state_header": {
    "schema_version": number,
    "identity_principles_hash": string,
    "safety_posture": string,
    "current_objective": string,
    "open_loops": string[],
    "next_actions": string[],
    "commitments": string[],
    "recent_context_summary": string,
    "last_updated_at": string (RFC3339)
  },
  "memory_append": string[]
}

Do not include unknown keys.
Do not change immutable fields.
If uncertain, keep mutable values close to current state."#;

fn build_reflect_message(
    canonical_state: Option<&StateHeaderV1>,
    user_message: &str,
    answer: &str,
) -> Result<String> {
    let canonical_json = match canonical_state {
        Some(state) => serde_json::to_string_pretty(state)?,
        None => "null".to_string(),
    };

    Ok(format!(
        "Current canonical state header (JSON):\n{canonical_json}\n\nLatest user message:\n{user_message}\n\nLatest assistant answer:\n{answer}\n\nReturn only the strict JSON payload."
    ))
}

fn parse_reflect_payload(raw: &str) -> Result<Value> {
    let payload: Value = serde_json::from_str(raw.trim())?;
    if !payload.is_object() {
        anyhow::bail!("reflect output must be a JSON object");
    }
    Ok(payload)
}

async fn run_persona_reflect_writeback(
    config: &Config,
    mem: Arc<dyn Memory>,
    provider: &dyn Provider,
    model_name: &str,
    user_message: &str,
    answer: &str,
) -> Result<()> {
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
    );

    let canonical_state = persistence.load_backend_canonical().await?;
    let reflect_message = build_reflect_message(canonical_state.as_ref(), user_message, answer)?;

    let reflect_raw = provider
        .chat_with_system(
            Some(PERSONA_REFLECT_SYSTEM_PROMPT),
            &reflect_message,
            model_name,
            0.0,
        )
        .await?;
    let reflect_payload = parse_reflect_payload(&reflect_raw)?;

    let Some(previous_state) = canonical_state else {
        tracing::warn!("persona reflect produced payload but canonical state header is missing");
        return Ok(());
    };

    let immutable = ImmutableStateHeader {
        schema_version: u32::from(previous_state.schema_version),
        identity_principles_hash: previous_state.identity_principles_hash.clone(),
        safety_posture: previous_state.safety_posture.clone(),
    };

    let accepted = match validate_writeback_payload(&reflect_payload, &immutable) {
        WritebackGuardVerdict::Accepted(payload) => payload,
        WritebackGuardVerdict::Rejected { reason } => {
            tracing::warn!(reason, "persona writeback rejected by guard");
            return Ok(());
        }
    };

    let candidate = StateHeaderV1 {
        schema_version: previous_state.schema_version,
        identity_principles_hash: previous_state.identity_principles_hash.clone(),
        safety_posture: previous_state.safety_posture.clone(),
        current_objective: accepted.state_header.current_objective,
        open_loops: accepted.state_header.open_loops,
        next_actions: accepted.state_header.next_actions,
        commitments: accepted.state_header.commitments,
        recent_context_summary: accepted.state_header.recent_context_summary,
        last_updated_at: accepted.state_header.last_updated_at,
    };

    StateHeaderV1::validate_writeback_candidate(&previous_state, &candidate, &config.persona)?;
    persistence
        .persist_backend_canonical_and_sync_mirror(&candidate)
        .await?;

    for (idx, entry) in accepted.memory_append.iter().enumerate() {
        let input = MemoryEventInput::new(
            "default",
            format!("persona.writeback.{idx}"),
            MemoryEventType::SummaryCompacted,
            entry.clone(),
            MemorySource::System,
            PrivacyLevel::Private,
        )
        .with_confidence(0.9)
        .with_importance(0.8)
        .with_occurred_at(candidate.last_updated_at.clone());
        mem.append_event(input).await?;
    }

    Ok(())
}

async fn execute_main_session_turn(
    config: &Config,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
) -> Result<String> {
    let outcome =
        execute_main_session_turn_with_accounting(config, mem, params, user_message).await?;
    Ok(outcome.response)
}

async fn execute_main_session_turn_with_accounting(
    config: &Config,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
) -> Result<TurnExecutionOutcome> {
    let mut accounting = TurnCallAccounting::for_persona_mode(config.persona.enabled_main_session);

    if config.memory.auto_save {
        let _ = mem
            .append_event(
                MemoryEventInput::new(
                    "default",
                    "conversation.user_msg",
                    MemoryEventType::FactAdded,
                    user_message,
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_confidence(0.95)
                .with_importance(0.6),
            )
            .await;
    }

    let context = build_context(mem.as_ref(), user_message).await;
    let enriched = if context.is_empty() {
        user_message.to_string()
    } else {
        format!("{context}{user_message}")
    };

    accounting.consume_answer_call()?;
    let response = params
        .answer_provider
        .chat_with_system(
            Some(params.system_prompt),
            &enriched,
            params.model_name,
            params.temperature,
        )
        .await?;

    if config.persona.enabled_main_session {
        accounting.consume_reflect_call()?;
        if let Err(error) = run_persona_reflect_writeback(
            config,
            mem.clone(),
            params.reflect_provider,
            params.model_name,
            user_message,
            &response,
        )
        .await
        {
            tracing::warn!(error = %error, "persona reflect/writeback failed; answer path preserved");
        }
    }

    if config.memory.auto_save {
        let summary = truncate_with_ellipsis(&response, 100);
        let _ = mem
            .append_event(
                MemoryEventInput::new(
                    "default",
                    "conversation.assistant_resp",
                    MemoryEventType::FactAdded,
                    summary,
                    MemorySource::System,
                    PrivacyLevel::Private,
                )
                .with_confidence(0.9)
                .with_importance(0.4),
            )
            .await;
    }

    Ok(TurnExecutionOutcome {
        response,
        accounting,
    })
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    config: Config,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    // ── Wire up agnostic subsystems ──────────────────────────────
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let _runtime = runtime::create_runtime(&config.runtime)?;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));

    // ── Memory (the brain) ────────────────────────────────────────
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);
    tracing::info!(backend = mem.name(), "Memory initialized");

    // ── Tools (including memory tools) ────────────────────────────
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let _tools = tools::all_tools(&security, mem.clone(), composio_key, &config.browser);

    // ── Resolve provider ─────────────────────────────────────────
    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");

    let model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");

    let answer_provider: Box<dyn Provider> = providers::create_resilient_provider(
        provider_name,
        config.api_key.as_deref(),
        &config.reliability,
    )?;
    let reflect_provider: Box<dyn Provider> =
        providers::create_provider(provider_name, config.api_key.as_deref())?;

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    // ── Build system prompt from workspace MD files (OpenClaw framework) ──
    let skills = crate::skills::load_skills(&config.workspace_dir);
    let mut tool_descs: Vec<(&str, &str)> = vec![
        (
            "shell",
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
        ),
        (
            "file_read",
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
        ),
        (
            "file_write",
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
        ),
        (
            "memory_store",
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
        ),
        (
            "memory_recall",
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
        ),
        (
            "memory_forget",
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
        ),
    ];
    if config.browser.enabled {
        tool_descs.push((
            "browser_open",
            "Open approved HTTPS URLs in Brave Browser (allowlist-only, no scraping)",
        ));
    }
    if config.composio.enabled {
        tool_descs.push((
            "composio",
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run, 'connect' to OAuth.",
        ));
    }
    let prompt_options = crate::channels::SystemPromptOptions {
        persona_state_mirror_filename: if config.persona.enabled_main_session {
            Some(config.persona.state_mirror_filename.clone())
        } else {
            None
        },
    };
    let system_prompt = crate::channels::build_system_prompt_with_options(
        &config.workspace_dir,
        model_name,
        &tool_descs,
        &skills,
        &prompt_options,
    );
    let turn_params = MainSessionTurnParams {
        answer_provider: answer_provider.as_ref(),
        reflect_provider: reflect_provider.as_ref(),
        system_prompt: &system_prompt,
        model_name,
        temperature,
    };

    // ── Execute ──────────────────────────────────────────────────
    let start = Instant::now();

    if let Some(msg) = message {
        let response = execute_main_session_turn(&config, mem.clone(), &turn_params, &msg).await?;
        println!("{response}");
    } else {
        println!("🦀 AsteronIris Interactive Mode");
        println!("Type /quit to exit.\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::channels::CliChannel::new();

        // Spawn listener
        let listen_handle = tokio::spawn(async move {
            let _ = crate::channels::Channel::listen(&cli, tx).await;
        });

        while let Some(msg) = rx.recv().await {
            let response =
                execute_main_session_turn(&config, mem.clone(), &turn_params, &msg.content).await?;
            println!("\n{response}\n");
        }

        listen_handle.abort();
    }

    let duration = start.elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        duration,
        tokens_used: None,
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PersonaConfig;
    use crate::memory::SqliteMemory;
    use crate::providers::reliable::ReliableProvider;
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    struct MockProvider {
        calls: Arc<AtomicUsize>,
        responses: Vec<String>,
        fail_on_call: Option<usize>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            let call_number = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_on_call == Some(call_number) {
                anyhow::bail!("mock failure on call {call_number}");
            }

            Ok(self
                .responses
                .get(call_number - 1)
                .cloned()
                .unwrap_or_else(|| "{}".to_string()))
        }
    }

    fn sample_state() -> StateHeaderV1 {
        StateHeaderV1 {
            schema_version: 1,
            identity_principles_hash: "identity-v1-abcd1234".to_string(),
            safety_posture: "strict".to_string(),
            current_objective: "Ship two-call main-session loop".to_string(),
            open_loops: vec!["Wire persona reflect stage".to_string()],
            next_actions: vec!["Add strict payload parsing".to_string()],
            commitments: vec!["Preserve answer path on call-2 failure".to_string()],
            recent_context_summary: "Task 4 integrates answer + reflect/writeback calls."
                .to_string(),
            last_updated_at: Utc::now().to_rfc3339(),
        }
    }

    fn build_reflect_payload(previous: &StateHeaderV1) -> String {
        json!({
            "state_header": {
                "schema_version": previous.schema_version,
                "identity_principles_hash": previous.identity_principles_hash,
                "safety_posture": previous.safety_posture,
                "current_objective": "Confirm two provider calls per turn",
                "open_loops": ["Validate call count invariant"],
                "next_actions": ["Run targeted persona loop tests"],
                "commitments": ["Keep main-session scope only"],
                "recent_context_summary": "Call-2 writes guarded payload with strict JSON parsing.",
                "last_updated_at": "2026-02-16T11:00:00Z"
            },
            "memory_append": ["persona loop writeback accepted"]
        })
        .to_string()
    }

    fn test_config(workspace_dir: &std::path::Path) -> Config {
        let mut config = Config::default();
        config.workspace_dir = workspace_dir.to_path_buf();
        config.memory.auto_save = false;
        config.persona = PersonaConfig {
            enabled_main_session: true,
            ..PersonaConfig::default()
        };
        config
    }

    #[tokio::test]
    async fn persona_loop_two_calls_per_turn() {
        let temp = TempDir::new().unwrap();
        let config = test_config(temp.path());

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let persistence = BackendCanonicalStateHeaderPersistence::new(
            mem.clone(),
            config.workspace_dir.clone(),
            config.persona.clone(),
        );
        let initial = sample_state();
        persistence
            .persist_backend_canonical_and_sync_mirror(&initial)
            .await
            .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockProvider {
            calls: calls.clone(),
            responses: vec![
                "answer-call-output".to_string(),
                build_reflect_payload(&initial),
            ],
            fail_on_call: None,
        };

        let response = execute_main_session_turn(
            &config,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.4,
            },
            "How do we wire Task 4?",
        )
        .await
        .unwrap();

        assert_eq!(response, "answer-call-output");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let updated = persistence.load_backend_canonical().await.unwrap().unwrap();
        assert_eq!(
            updated.current_objective,
            "Confirm two provider calls per turn"
        );
    }

    #[tokio::test]
    async fn persona_loop_call2_failure_preserves_answer() {
        let temp = TempDir::new().unwrap();
        let config = test_config(temp.path());

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let persistence = BackendCanonicalStateHeaderPersistence::new(
            mem.clone(),
            config.workspace_dir.clone(),
            config.persona.clone(),
        );
        let initial = sample_state();
        persistence
            .persist_backend_canonical_and_sync_mirror(&initial)
            .await
            .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockProvider {
            calls: calls.clone(),
            responses: vec!["answer-survives-call2-failure".to_string()],
            fail_on_call: Some(2),
        };

        let response = execute_main_session_turn(
            &config,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.4,
            },
            "Keep answer path stable",
        )
        .await
        .unwrap();

        assert_eq!(response, "answer-survives-call2-failure");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let persisted = persistence.load_backend_canonical().await.unwrap().unwrap();
        assert_eq!(persisted, initial);
    }

    struct AlwaysFailProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for AlwaysFailProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("transient reflect failure")
        }
    }

    #[tokio::test]
    async fn persona_reflect_no_retry() {
        let temp = TempDir::new().unwrap();
        let config = test_config(temp.path());

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let persistence = BackendCanonicalStateHeaderPersistence::new(
            mem.clone(),
            config.workspace_dir.clone(),
            config.persona.clone(),
        );
        let initial = sample_state();
        persistence
            .persist_backend_canonical_and_sync_mirror(&initial)
            .await
            .unwrap();

        let answer_calls = Arc::new(AtomicUsize::new(0));
        let answer_provider = ReliableProvider::new(
            vec![(
                "primary".to_string(),
                Box::new(MockProvider {
                    calls: answer_calls.clone(),
                    responses: vec![
                        "unused-first-attempt".to_string(),
                        "answer-with-reliable-configured".to_string(),
                    ],
                    fail_on_call: Some(1),
                }),
            )],
            3,
            1,
        );

        let reflect_calls = Arc::new(AtomicUsize::new(0));
        let reflect_provider = AlwaysFailProvider {
            calls: reflect_calls.clone(),
        };

        let response = execute_main_session_turn(
            &config,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &answer_provider,
                reflect_provider: &reflect_provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.2,
            },
            "verify reflect retry suppression",
        )
        .await
        .unwrap();

        assert_eq!(response, "answer-with-reliable-configured");
        assert_eq!(answer_calls.load(Ordering::SeqCst), 2);
        assert_eq!(reflect_calls.load(Ordering::SeqCst), 1);

        let persisted = persistence.load_backend_canonical().await.unwrap().unwrap();
        assert_eq!(persisted, initial);
    }

    #[tokio::test]
    async fn persona_budget_counter_stable() {
        let temp = TempDir::new().unwrap();
        let config = test_config(temp.path());

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let persistence = BackendCanonicalStateHeaderPersistence::new(
            mem.clone(),
            config.workspace_dir.clone(),
            config.persona.clone(),
        );
        let initial = sample_state();
        persistence
            .persist_backend_canonical_and_sync_mirror(&initial)
            .await
            .unwrap();

        let answer_calls = Arc::new(AtomicUsize::new(0));
        let answer_provider = MockProvider {
            calls: answer_calls.clone(),
            responses: vec![
                "turn-1-answer".to_string(),
                "turn-2-answer".to_string(),
                "turn-3-answer".to_string(),
            ],
            fail_on_call: None,
        };

        let reflect_calls = Arc::new(AtomicUsize::new(0));
        let reflect_provider = ReliableProvider::new(
            vec![(
                "primary".to_string(),
                Box::new(MockProvider {
                    calls: reflect_calls.clone(),
                    responses: vec![
                        build_reflect_payload(&initial),
                        build_reflect_payload(&initial),
                        build_reflect_payload(&initial),
                    ],
                    fail_on_call: None,
                }),
            )],
            3,
            1,
        );

        for turn in 0..3 {
            let outcome = execute_main_session_turn_with_accounting(
                &config,
                mem.clone(),
                &MainSessionTurnParams {
                    answer_provider: &answer_provider,
                    reflect_provider: &reflect_provider,
                    system_prompt: "system",
                    model_name: "test-model",
                    temperature: 0.2,
                },
                &format!("turn-{turn}-message"),
            )
            .await
            .unwrap();

            assert_eq!(
                outcome.accounting.budget_limit,
                PERSONA_PER_TURN_CALL_BUDGET
            );
            assert_eq!(outcome.accounting.answer_calls, 1);
            assert_eq!(outcome.accounting.reflect_calls, 1);
            assert_eq!(outcome.accounting.total_calls(), 2);
        }

        assert_eq!(answer_calls.load(Ordering::SeqCst), 3);
        assert_eq!(reflect_calls.load(Ordering::SeqCst), 3);
    }
}
