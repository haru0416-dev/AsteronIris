use super::context::build_context_with_policy;
use super::inference::run_post_turn_inference_pass;
use super::reflect::run_persona_reflect_writeback;
use super::types::{
    MainSessionTurnParams, RuntimeMemoryWriteContext, TurnCallAccounting, TurnExecutionOutcome,
};
use super::verify_repair::{
    VerifyRepairCaps, analyze_verify_failure, decide_verify_repair_escalation,
    emit_verify_repair_escalation_event,
};

use crate::config::Config;
use crate::intelligence::agent::tool_loop::{LoopStopReason, ToolLoop};
use crate::memory::traits::MemoryLayer;
use crate::memory::{
    self, Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
};
use crate::observability::traits::AutonomyLifecycleSignal;
use crate::observability::{NoopObserver, Observer};
use crate::providers::{CliStreamSink, Provider, StreamSink};
use crate::security::policy::{EntityRateLimiter, TenantPolicyContext};
use crate::security::{PermissionStore, SecurityPolicy};
use crate::tools;
use crate::tools::middleware::ExecutionContext;
use crate::util::truncate_with_ellipsis;
use anyhow::{Context, Result};
use std::sync::Arc;

#[cfg(test)]
pub(super) async fn execute_main_session_turn(
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

pub(super) async fn execute_main_session_turn_with_metrics(
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
            Arc::clone(&mem),
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
    let security_arc = Arc::new(security.clone());
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools = tools::all_tools(
        &security_arc,
        Arc::clone(&mem),
        composio_key,
        &config.browser,
        &config.tools,
        Some(&config.mcp),
    );
    let middleware = tools::default_middleware_chain();
    let mut registry = tools::ToolRegistry::new(middleware);
    for tool in tools {
        registry.register(tool);
    }
    let params = MainSessionTurnParams {
        answer_provider,
        reflect_provider,
        system_prompt,
        model_name,
        temperature,
        registry: Arc::new(registry),
        max_tool_iterations: config.autonomy.max_tool_loop_iterations,
        rate_limiter: Arc::new(EntityRateLimiter::new(
            config.autonomy.max_actions_per_hour,
            config.autonomy.max_actions_per_entity_per_hour,
        )),
        permission_store: Arc::new(PermissionStore::load(&config.workspace_dir)),
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
pub(super) async fn execute_main_session_turn_with_accounting(
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

    save_user_message_if_enabled(config, mem.as_ref(), write_context, user_message).await;

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
    accounting
        .consume_answer_call()
        .context("consume answer call budget")?;
    let effective_autonomy_level = config.autonomy.effective_autonomy_level();
    let requested_temperature = params.temperature;
    let clamped_temperature = config.autonomy.clamp_temperature(requested_temperature);
    if (requested_temperature - clamped_temperature).abs() > f64::EPSILON {
        let band = config.autonomy.selected_temperature_band();
        tracing::info!(
            autonomy_level = ?effective_autonomy_level,
            requested_temperature,
            clamped_temperature,
            band_min = band.min,
            band_max = band.max,
            "temperature clamped to autonomy band"
        );
    }
    let tool_loop = ToolLoop::new(Arc::clone(&params.registry), params.max_tool_iterations);
    let ctx = ExecutionContext {
        security: Arc::new(security.clone()),
        autonomy_level: effective_autonomy_level,
        entity_id: "cli:local".to_string(),
        turn_number: 0,
        workspace_dir: config.workspace_dir.clone(),
        allowed_tools: None,
        permission_store: Some(Arc::clone(&params.permission_store)),
        rate_limiter: Arc::clone(&params.rate_limiter),
        tenant_context: TenantPolicyContext::disabled(),
        approval_broker: None,
    };
    let tool_result = tool_loop
        .run(
            params.answer_provider,
            params.system_prompt,
            &enriched,
            &[],
            params.model_name,
            clamped_temperature,
            &ctx,
            Some(Arc::new(CliStreamSink::new()) as Arc<dyn StreamSink>),
        )
        .await
        .context("run agent tool loop")?;
    tracing::debug!(
        entity_id = %ctx.entity_id,
        iterations = tool_result.iterations,
        stop_reason = ?tool_result.stop_reason,
        "main session tool loop completed"
    );
    match &tool_result.stop_reason {
        LoopStopReason::Completed => {}
        LoopStopReason::MaxIterations => {
            tracing::warn!(
                iterations = tool_result.iterations,
                "tool loop hit max iterations"
            );
        }
        LoopStopReason::RateLimited => {
            tracing::warn!("tool loop halted by rate limiter");
        }
        LoopStopReason::ApprovalDenied => {
            tracing::warn!("tool loop halted by approval requirement");
        }
        LoopStopReason::Error(message) => {
            anyhow::bail!("tool loop failed: {message}");
        }
    }
    let tokens_used = tool_result.tokens_used;
    let response = tool_result.final_text;

    if config.persona.enabled_main_session {
        security
            .consume_action_and_cost(0)
            .map_err(anyhow::Error::msg)
            .context("consume rate limit for persona reflect")?;
        accounting
            .consume_reflect_call()
            .context("consume reflect call budget")?;
        if let Err(error) = run_persona_reflect_writeback(
            config,
            Arc::clone(&mem),
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

    save_response_and_consolidate(
        config,
        &mem,
        write_context,
        user_message,
        &response,
        observer,
    )
    .await;

    Ok(TurnExecutionOutcome {
        response,
        tokens_used,
        accounting,
    })
}

async fn save_user_message_if_enabled(
    config: &Config,
    mem: &dyn Memory,
    write_context: &RuntimeMemoryWriteContext,
    user_message: &str,
) {
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
}

async fn save_response_and_consolidate(
    config: &Config,
    mem: &Arc<dyn Memory>,
    write_context: &RuntimeMemoryWriteContext,
    user_message: &str,
    response: &str,
    observer: &Arc<dyn Observer>,
) {
    if !config.memory.auto_save {
        return;
    }

    let summary = truncate_with_ellipsis(response, 100);
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
        run_post_turn_inference_pass(mem.as_ref(), write_context, response, observer).await
    {
        tracing::warn!(error = %error, "post-turn memory inference pass failed");
    }

    match mem.count_events(Some(&write_context.entity_id)).await {
        Ok(checkpoint_event_count) => {
            let input = memory::ConsolidationInput::new(
                &write_context.entity_id,
                checkpoint_event_count,
                user_message,
                response,
            );
            memory::enqueue_consolidation_task(
                Arc::clone(mem),
                config.workspace_dir.clone(),
                input,
                Arc::clone(observer),
            );
        }
        Err(error) => {
            tracing::warn!(error = %error, "post-turn consolidation checkpoint skipped");
        }
    }
}
