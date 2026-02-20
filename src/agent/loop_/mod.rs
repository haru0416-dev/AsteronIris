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
    VerifyRepairCaps, analyze_verify_failure, decide_verify_repair_escalation,
    emit_verify_repair_escalation_event,
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
use crate::security::SecurityPolicy;
use crate::security::policy::TenantPolicyContext;
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
    tokens_used: Option<u64>,
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

#[cfg(test)]
async fn execute_main_session_turn(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    observer: &Arc<dyn Observer>,
) -> Result<String> {
    execute_main_session_turn_with_metrics(config, security, mem, params, user_message, observer)
        .await
        .map(|outcome| outcome.response)
}

async fn execute_main_session_turn_with_metrics(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    observer: &Arc<dyn Observer>,
) -> Result<TurnExecutionOutcome> {
    execute_main_session_turn_with_policy_outcome(
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

async fn execute_main_session_turn_with_policy_outcome(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    write_context: RuntimeMemoryWriteContext,
    observer: &Arc<dyn Observer>,
) -> Result<TurnExecutionOutcome> {
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
            Ok(outcome) => return Ok(outcome),
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

async fn execute_main_session_turn_with_policy(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    write_context: RuntimeMemoryWriteContext,
    observer: &Arc<dyn Observer>,
) -> Result<String> {
    execute_main_session_turn_with_policy_outcome(
        config,
        security,
        mem,
        params,
        user_message,
        write_context,
        observer,
    )
    .await
    .map(|outcome| outcome.response)
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
    let response_full = params
        .answer_provider
        .chat_with_system_full(
            Some(params.system_prompt),
            &enriched,
            params.model_name,
            clamped_temperature,
        )
        .await?;
    let tokens_used = response_full.total_tokens();
    let response = response_full.text;

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
        tokens_used,
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
    let tool_descs =
        crate::tools::tool_descriptions(config.browser.enabled, config.composio.enabled);
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
    let mut token_sum = 0_u64;
    let mut saw_token_usage = false;

    if let Some(msg) = message {
        let outcome = execute_main_session_turn_with_metrics(
            &config,
            security.as_ref(),
            mem.clone(),
            &turn_params,
            &msg,
            &observer,
        )
        .await?;
        if let Some(tokens) = outcome.tokens_used {
            token_sum = token_sum.saturating_add(tokens);
            saw_token_usage = true;
        }
        println!("{}", outcome.response);
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
            let outcome = execute_main_session_turn_with_metrics(
                &config,
                security.as_ref(),
                mem.clone(),
                &turn_params,
                &msg.content,
                &observer,
            )
            .await?;
            if let Some(tokens) = outcome.tokens_used {
                token_sum = token_sum.saturating_add(tokens);
                saw_token_usage = true;
            }
            println!("\n{}\n", outcome.response);
        }

        listen_handle.abort();
    }

    let duration = start.elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        duration,
        tokens_used: saw_token_usage.then_some(token_sum),
    });

    Ok(())
}

#[cfg(test)]
mod tests;
