mod context;
mod inference;
mod reflect;
mod verify_repair;

#[allow(unused_imports)]
pub use context::build_context_for_integration;

use context::build_context_with_policy;
use inference::run_post_turn_inference_pass;
use reflect::run_persona_reflect_writeback;
use verify_repair::{
    analyze_verify_failure, decide_verify_repair_escalation, emit_verify_repair_escalation_event,
    VerifyRepairCaps,
};

use crate::auth::AuthBroker;
use crate::config::Config;
use crate::memory::traits::MemoryLayer;
use crate::memory::{
    self, Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
};
use crate::observability::traits::AutonomyLifecycleSignal;
use crate::observability::{self, NoopObserver, Observer, ObserverEvent};
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::policy::TenantPolicyContext;
use crate::security::SecurityPolicy;
use crate::tools;
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
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

#[derive(Debug, Clone)]
pub(super) struct RuntimeMemoryWriteContext {
    pub(super) entity_id: String,
    pub(super) policy_context: TenantPolicyContext,
}

impl RuntimeMemoryWriteContext {
    fn main_session_default() -> Self {
        Self {
            entity_id: "default".to_string(),
            policy_context: TenantPolicyContext::disabled(),
        }
    }

    fn for_entity_with_policy(
        entity_id: impl Into<String>,
        policy_context: TenantPolicyContext,
    ) -> Self {
        Self {
            entity_id: entity_id.into(),
            policy_context,
        }
    }

    pub(super) fn enforce_write_scope(&self) -> Result<()> {
        self.policy_context
            .enforce_recall_scope(&self.entity_id)
            .map_err(anyhow::Error::msg)
    }
}

async fn execute_main_session_turn(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    observer: &Arc<dyn Observer>,
) -> Result<String> {
    execute_main_session_turn_with_policy(
        config,
        security,
        mem,
        params,
        user_message,
        RuntimeMemoryWriteContext::main_session_default(),
        observer,
    )
    .await
}

async fn execute_main_session_turn_with_policy(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    write_context: RuntimeMemoryWriteContext,
    observer: &Arc<dyn Observer>,
) -> Result<String> {
    let caps = VerifyRepairCaps::from_config(config);
    let mut attempts = 0_u32;
    let mut repair_depth = 0_u32;

    loop {
        attempts = attempts.saturating_add(1);
        match execute_main_session_turn_with_accounting(
            config,
            security,
            mem.clone(),
            params,
            user_message,
            &write_context,
            observer,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome.response),
            Err(error) => {
                let analysis = analyze_verify_failure(&error);
                if let Some(escalation) =
                    decide_verify_repair_escalation(caps, attempts, repair_depth, analysis, &error)
                {
                    if let Err(event_error) =
                        emit_verify_repair_escalation_event(mem.as_ref(), &escalation).await
                    {
                        tracing::warn!(
                            error = %event_error,
                            "verify/repair escalation event write failed"
                        );
                    }
                    anyhow::bail!(escalation.contract_message());
                }

                repair_depth = repair_depth.saturating_add(1);
                tracing::warn!(
                    attempt = attempts,
                    repair_depth,
                    failure_class = analysis.failure_class,
                    retryable = analysis.retryable,
                    error = %error,
                    "verify/repair retrying turn"
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_main_session_turn_for_integration(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    answer_provider: &dyn Provider,
    reflect_provider: &dyn Provider,
    system_prompt: &str,
    model_name: &str,
    temperature: f64,
    user_message: &str,
) -> Result<String> {
    run_main_session_turn_for_integration_with_policy(
        config,
        security,
        mem,
        answer_provider,
        reflect_provider,
        system_prompt,
        model_name,
        temperature,
        "default",
        TenantPolicyContext::disabled(),
        user_message,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_main_session_turn_for_integration_with_policy(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    answer_provider: &dyn Provider,
    reflect_provider: &dyn Provider,
    system_prompt: &str,
    model_name: &str,
    temperature: f64,
    entity_id: &str,
    policy_context: TenantPolicyContext,
    user_message: &str,
) -> Result<String> {
    let observer: Arc<dyn Observer> = Arc::new(NoopObserver);
    let params = MainSessionTurnParams {
        answer_provider,
        reflect_provider,
        system_prompt,
        model_name,
        temperature,
    };

    execute_main_session_turn_with_policy(
        config,
        security,
        mem,
        &params,
        user_message,
        RuntimeMemoryWriteContext::for_entity_with_policy(entity_id, policy_context),
        &observer,
    )
    .await
}

#[allow(clippy::too_many_lines)]
async fn execute_main_session_turn_with_accounting(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    write_context: &RuntimeMemoryWriteContext,
    observer: &Arc<dyn Observer>,
) -> Result<TurnExecutionOutcome> {
    observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::IntentCreated);
    let mut accounting = TurnCallAccounting::for_persona_mode(config.persona.enabled_main_session);
    write_context.enforce_write_scope()?;

    if config.memory.auto_save {
        let _ = mem
            .append_event(
                MemoryEventInput::new(
                    &write_context.entity_id,
                    "conversation.user_msg",
                    MemoryEventType::FactAdded,
                    user_message,
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working)
                .with_confidence(0.95)
                .with_importance(0.6)
                .with_provenance(MemoryProvenance::source_reference(
                    MemorySource::ExplicitUser,
                    "agent.autosave.user_msg",
                )),
            )
            .await;
    }

    let context = build_context_with_policy(
        mem.as_ref(),
        &write_context.entity_id,
        user_message,
        write_context.policy_context.clone(),
    )
    .await
    .unwrap_or_default();
    let enriched = if context.is_empty() {
        user_message.to_string()
    } else {
        format!("{context}{user_message}")
    };

    match security.consume_action_and_cost(0) {
        Ok(()) => {
            observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::IntentPolicyAllowed);
        }
        Err(e) => {
            observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::IntentPolicyDenied);
            return Err(anyhow::Error::msg(e));
        }
    }
    accounting.consume_answer_call()?;
    let requested_temperature = params.temperature;
    let clamped_temperature = config.autonomy.clamp_temperature(requested_temperature);
    if (requested_temperature - clamped_temperature).abs() > f64::EPSILON {
        let band = config.autonomy.selected_temperature_band();
        tracing::info!(
            autonomy_level = ?config.autonomy.level,
            requested_temperature,
            clamped_temperature,
            band_min = band.min,
            band_max = band.max,
            "temperature clamped to autonomy band"
        );
    }
    let response = params
        .answer_provider
        .chat_with_system(
            Some(params.system_prompt),
            &enriched,
            params.model_name,
            clamped_temperature,
        )
        .await?;

    if config.persona.enabled_main_session {
        security
            .consume_action_and_cost(0)
            .map_err(anyhow::Error::msg)?;
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
                    &write_context.entity_id,
                    "conversation.assistant_resp",
                    MemoryEventType::FactAdded,
                    summary,
                    MemorySource::System,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working)
                .with_confidence(0.9)
                .with_importance(0.4)
                .with_provenance(MemoryProvenance::source_reference(
                    MemorySource::System,
                    "agent.autosave.assistant_resp",
                )),
            )
            .await;

        if let Err(error) =
            run_post_turn_inference_pass(mem.as_ref(), write_context, &response, observer).await
        {
            tracing::warn!(error = %error, "post-turn memory inference pass failed");
        }

        match mem.count_events(Some(&write_context.entity_id)).await {
            Ok(checkpoint_event_count) => {
                let input = memory::ConsolidationInput::new(
                    &write_context.entity_id,
                    checkpoint_event_count,
                    user_message,
                    &response,
                );
                memory::enqueue_consolidation_task(
                    mem.clone(),
                    config.workspace_dir.clone(),
                    input,
                    observer.clone(),
                );
            }
            Err(error) => {
                tracing::warn!(error = %error, "post-turn consolidation checkpoint skipped");
            }
        }
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
    let auth_broker = AuthBroker::load_or_init(&config)?;

    // ── Memory (the brain) ────────────────────────────────────────
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        memory_api_key.as_deref(),
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

    let answer_provider: Box<dyn Provider> =
        providers::create_resilient_provider_with_oauth_recovery(
            &config,
            provider_name,
            &config.reliability,
            |name| auth_broker.resolve_provider_api_key(name),
        )?;
    let reflect_api_key = auth_broker.resolve_provider_api_key(provider_name);
    let reflect_provider: Box<dyn Provider> = providers::create_provider_with_oauth_recovery(
        &config,
        provider_name,
        reflect_api_key.as_deref(),
    )?;

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
        let response = execute_main_session_turn(
            &config,
            security.as_ref(),
            mem.clone(),
            &turn_params,
            &msg,
            &observer,
        )
        .await?;
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
            let response = execute_main_session_turn(
                &config,
                security.as_ref(),
                mem.clone(),
                &turn_params,
                &msg.content,
                &observer,
            )
            .await?;
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
    use crate::persona::state_header::StateHeaderV1;
    use crate::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
    use crate::providers::reliable::ReliableProvider;
    use crate::security::SecurityPolicy;
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tempfile::TempDir;
    use verify_repair::VERIFY_REPAIR_ESCALATION_SLOT_KEY;

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

    #[allow(clippy::field_reassign_with_default)]
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

    fn noop_observer() -> Arc<dyn Observer> {
        Arc::new(NoopObserver)
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
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let response = execute_main_session_turn(
            &config,
            &security,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.4,
            },
            "How do we wire Task 4?",
            &noop_observer(),
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
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let response = execute_main_session_turn(
            &config,
            &security,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.4,
            },
            "Keep answer path stable",
            &noop_observer(),
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

    struct TemperatureCaptureProvider {
        temperatures: Arc<Mutex<Vec<f64>>>,
        response: String,
    }

    struct MessageFailProvider {
        calls: Arc<AtomicUsize>,
        message: &'static str,
    }

    #[async_trait]
    impl Provider for TemperatureCaptureProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            temperature: f64,
        ) -> anyhow::Result<String> {
            self.temperatures
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(temperature);
            Ok(self.response.clone())
        }
    }

    #[async_trait]
    impl Provider for MessageFailProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!(self.message)
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
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let response = execute_main_session_turn(
            &config,
            &security,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &answer_provider,
                reflect_provider: &reflect_provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.2,
            },
            "verify reflect retry suppression",
            &noop_observer(),
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
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        for turn in 0..3 {
            let outcome = execute_main_session_turn_with_accounting(
                &config,
                &security,
                mem.clone(),
                &MainSessionTurnParams {
                    answer_provider: &answer_provider,
                    reflect_provider: &reflect_provider,
                    system_prompt: "system",
                    model_name: "test-model",
                    temperature: 0.2,
                },
                &format!("turn-{turn}-message"),
                &RuntimeMemoryWriteContext::main_session_default(),
                &noop_observer(),
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

    #[tokio::test]
    async fn persona_loop_policy_blocks_when_action_limit_is_exhausted() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.autonomy.max_actions_per_hour = 0;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockProvider {
            calls: calls.clone(),
            responses: vec!["should-not-run".to_string()],
            fail_on_call: None,
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let err = execute_main_session_turn(
            &config,
            &security,
            mem,
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.1,
            },
            "blocked by policy",
            &noop_observer(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("action limit exceeded"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn autonomy_temperature_clamped() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.persona.enabled_main_session = false;
        config.autonomy.level = crate::security::AutonomyLevel::Full;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let temperatures = Arc::new(Mutex::new(Vec::new()));
        let provider = TemperatureCaptureProvider {
            temperatures: temperatures.clone(),
            response: "clamped-temp-response".to_string(),
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let response = execute_main_session_turn(
            &config,
            &security,
            mem,
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 1.9,
            },
            "clamp this temperature",
            &noop_observer(),
        )
        .await
        .unwrap();

        assert_eq!(response, "clamped-temp-response");
        let seen = temperatures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(seen, vec![1.0]);
    }

    #[tokio::test]
    async fn verify_repair_recovers_within_attempt_cap() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.persona.enabled_main_session = false;
        config.autonomy.verify_repair_max_attempts = 3;
        config.autonomy.verify_repair_max_repair_depth = 2;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockProvider {
            calls: calls.clone(),
            responses: vec![
                "unused-first-attempt".to_string(),
                "recovered-on-second-attempt".to_string(),
            ],
            fail_on_call: Some(1),
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let response = execute_main_session_turn(
            &config,
            &security,
            mem,
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.2,
            },
            "recover after one failure",
            &noop_observer(),
        )
        .await
        .unwrap();

        assert_eq!(response, "recovered-on-second-attempt");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn verify_repair_stops_at_max_attempts() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.persona.enabled_main_session = false;
        config.autonomy.verify_repair_max_attempts = 3;
        config.autonomy.verify_repair_max_repair_depth = 2;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MessageFailProvider {
            calls: calls.clone(),
            message: "deterministic transient failure",
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let err = execute_main_session_turn(
            &config,
            &security,
            mem,
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.2,
            },
            "always fail",
            &noop_observer(),
        )
        .await
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("reason=max_attempts_reached"));
        assert!(message.contains("attempts=3"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn verify_repair_emits_escalation_event() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.persona.enabled_main_session = false;
        config.autonomy.verify_repair_max_attempts = 2;
        config.autonomy.verify_repair_max_repair_depth = 1;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MessageFailProvider {
            calls: calls.clone(),
            message: "deterministic retry failure",
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let err = execute_main_session_turn(
            &config,
            &security,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.2,
            },
            "escalate and emit event",
            &noop_observer(),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("reason=max_attempts_reached"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let escalation = mem
            .resolve_slot("default", VERIFY_REPAIR_ESCALATION_SLOT_KEY)
            .await
            .unwrap()
            .expect("escalation event should be written");

        assert!(escalation
            .value
            .contains("\"reason\":\"max_attempts_reached\""));
        assert!(escalation.value.contains("\"attempts\":2"));
        assert!(escalation
            .value
            .contains("\"failure_class\":\"transient_failure\""));
    }

    #[tokio::test]
    async fn verify_repair_retries_still_enforce_policy_limits() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.persona.enabled_main_session = false;
        config.autonomy.max_actions_per_hour = 2;
        config.autonomy.verify_repair_max_attempts = 5;
        config.autonomy.verify_repair_max_repair_depth = 4;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MessageFailProvider {
            calls: calls.clone(),
            message: "retry until policy blocks",
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let err = execute_main_session_turn(
            &config,
            &security,
            mem,
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.2,
            },
            "policy must gate every retry",
            &noop_observer(),
        )
        .await
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("reason=non_retryable_failure"));
        assert!(message.contains("failure_class=policy_limit"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn post_turn_inference_hook_appends_tagged_events() {
        let temp = TempDir::new().unwrap();
        let mut config = test_config(temp.path());
        config.memory.auto_save = true;
        config.persona.enabled_main_session = false;

        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
        let provider = MockProvider {
            calls: Arc::new(AtomicUsize::new(0)),
            responses: vec![
                "INFERRED_CLAIM inference.preference.language => User prefers Rust\nCONTRADICTION_EVENT contradiction.preference.language => Earlier note said Python".to_string(),
            ],
            fail_on_call: None,
        };
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let response = execute_main_session_turn(
            &config,
            &security,
            mem.clone(),
            &MainSessionTurnParams {
                answer_provider: &provider,
                reflect_provider: &provider,
                system_prompt: "system",
                model_name: "test-model",
                temperature: 0.3,
            },
            "derive inferences",
            &noop_observer(),
        )
        .await
        .unwrap();
        assert!(response.contains("INFERRED_CLAIM"));

        let inferred = mem
            .resolve_slot("default", "inference.preference.language")
            .await
            .unwrap()
            .expect("inferred claim should persist");
        assert_eq!(inferred.source, MemorySource::Inferred);

        let contradiction = mem
            .resolve_slot("default", "contradiction.preference.language")
            .await
            .unwrap()
            .expect("contradiction event should be represented as event");
        assert_eq!(contradiction.source, MemorySource::System);
    }
}
