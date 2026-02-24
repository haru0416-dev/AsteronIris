use super::context::build_context_with_policy;
use super::inference::run_post_turn_inference_pass;
use super::reflect::run_persona_reflect_writeback;
use super::types::{
    IntegrationRuntimeTurnOptions, IntegrationTurnParams, MainSessionTurnParams,
    RuntimeMemoryWriteContext, TurnCallAccounting, TurnExecutionOutcome,
};
use super::verify_repair::{
    VerifyRepairCaps, analyze_verify_failure, decide_verify_repair_escalation,
    emit_verify_repair_escalation_event,
};

use crate::agent::{LoopStopReason, PromptHook, ToolLoop, ToolLoopResult, ToolLoopRunParams};
use crate::config::Config;
use crate::llm::ProviderMessage;
use crate::llm::{CliStreamSink, StreamSink};
use crate::memory::{
    self, Memory, MemoryEventInput, MemoryEventType, MemoryLayer, MemoryProvenance, MemorySource,
    PrivacyLevel, SourceKind,
};
use crate::persona::person_identity::resolve_person_id;
use crate::planner::{ExecutionReport, Plan, PlanExecutor, PlanParser, StepStatus, ToolStepRunner};
use crate::runtime::observability::traits::AutonomyLifecycleSignal;
use crate::runtime::observability::{NoopObserver, Observer};
use crate::security::SecurityPolicy;
use crate::security::policy::{AutonomyLevel, EntityRateLimiter, TenantPolicyContext};
use crate::security::writeback_guard::enforce_agent_autosave_write_policy;
use crate::tools::middleware::ExecutionContext;
use crate::utils::text::truncate_with_ellipsis;
use anyhow::{Context, Result};
use std::sync::Arc;

pub(super) struct MainSessionRuntimeOptions<'a> {
    execution_context_override: Option<ExecutionContext>,
    stream_sink: Option<Arc<dyn StreamSink>>,
    conversation_history: &'a [ProviderMessage],
    hooks: &'a [Arc<dyn PromptHook>],
}

#[cfg(test)]
#[allow(dead_code)]
pub(super) async fn execute_main_session_turn(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    observer: &Arc<dyn Observer>,
) -> Result<String> {
    execute_main_session_turn_with_metrics(
        config,
        security,
        mem,
        params,
        user_message,
        observer,
        &[],
    )
    .await
    .map(|outcome| outcome.response)
}

#[allow(dead_code)]
pub(super) async fn execute_main_session_turn_with_metrics(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    observer: &Arc<dyn Observer>,
    conversation_history: &[ProviderMessage],
) -> Result<TurnExecutionOutcome> {
    let runtime_options = MainSessionRuntimeOptions {
        execution_context_override: None,
        stream_sink: Some(Arc::new(CliStreamSink::new()) as Arc<dyn StreamSink>),
        conversation_history,
        hooks: &[],
    };
    execute_main_session_turn_with_policy_outcome(
        config,
        security,
        mem,
        params,
        user_message,
        RuntimeMemoryWriteContext::main_session_person(params.person_id),
        observer,
        &runtime_options,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_main_session_turn_with_policy_outcome(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    write_context: RuntimeMemoryWriteContext,
    observer: &Arc<dyn Observer>,
    runtime_options: &MainSessionRuntimeOptions<'_>,
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
            runtime_options,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(error) => {
                let analysis = analyze_verify_failure(&error);
                if let Some(escalation) =
                    decide_verify_repair_escalation(caps, attempts, repair_depth, analysis, &error)
                {
                    if let Err(event_error) = emit_verify_repair_escalation_event(
                        mem.as_ref(),
                        &write_context.entity_id,
                        &escalation,
                    )
                    .await
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
    let runtime_options = MainSessionRuntimeOptions {
        execution_context_override: None,
        stream_sink: Some(Arc::new(CliStreamSink::new()) as Arc<dyn StreamSink>),
        conversation_history: &[],
        hooks: &[],
    };
    execute_main_session_turn_with_policy_outcome(
        config,
        security,
        mem,
        params,
        user_message,
        write_context,
        observer,
        &runtime_options,
    )
    .await
    .map(|outcome| outcome.response)
}

fn enrich_user_message(context: &str, user_message: &str) -> String {
    if context.is_empty() {
        user_message.to_string()
    } else {
        format!("{context}{user_message}")
    }
}

fn build_main_session_execution_context(
    config: &Config,
    security: &SecurityPolicy,
    params: &MainSessionTurnParams<'_>,
    write_context: &RuntimeMemoryWriteContext,
    effective_autonomy_level: AutonomyLevel,
) -> ExecutionContext {
    ExecutionContext {
        security: Arc::new(security.clone()),
        autonomy_level: effective_autonomy_level,
        entity_id: write_context.entity_id.clone(),
        turn_number: 0,
        workspace_dir: config.workspace_dir.clone(),
        allowed_tools: None,
        rate_limiter: Arc::clone(&params.rate_limiter),
        tenant_context: write_context.policy_context.clone(),
    }
}

fn handle_tool_loop_stop_reason(stop_reason: &LoopStopReason, iterations: u32) -> Result<()> {
    match stop_reason {
        LoopStopReason::Completed => Ok(()),
        LoopStopReason::MaxIterations => {
            tracing::warn!(iterations, "tool loop hit max iterations");
            Ok(())
        }
        LoopStopReason::RateLimited => {
            tracing::warn!("tool loop halted by rate limiter");
            Ok(())
        }
        LoopStopReason::ApprovalDenied => {
            tracing::warn!("tool loop halted by approval requirement");
            Ok(())
        }
        LoopStopReason::Error(message) => anyhow::bail!("tool loop failed: {message}"),
        LoopStopReason::HookBlocked(reason) => {
            tracing::warn!(reason, "tool loop halted by prompt hook");
            Ok(())
        }
    }
}

fn should_attempt_planner(user_message: &str) -> bool {
    let lowercase = user_message.to_lowercase();
    let numbered_markers = ["1.", "2.", "3.", "1)", "2)", "3)"];
    let numbered_hits = numbered_markers
        .iter()
        .filter(|marker| lowercase.contains(*marker))
        .count();
    if numbered_hits >= 3 {
        return true;
    }

    let bullet_lines = user_message
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("- ") || trimmed.starts_with("* ")
        })
        .count();
    if bullet_lines >= 3 {
        return true;
    }

    let connector_tokens = [" then ", " next ", " after ", " finally "];
    let connector_hits = connector_tokens
        .iter()
        .filter(|token| lowercase.contains(*token))
        .count();
    connector_hits >= 2
}

fn build_planner_request(user_message: &str, tool_names: &[String]) -> String {
    let tool_list = if tool_names.is_empty() {
        "(no tools available)".to_string()
    } else {
        tool_names.join(", ")
    };

    format!(
        "You are the planning controller for an autonomous agent. Build a DAG plan with at least 3 steps for this task.\n\nAvailable tools: {tool_list}\n\n{}\n\nTask:\n{}",
        PlanParser::schema_prompt(),
        user_message
    )
}

fn final_step_output(plan: &Plan) -> Option<String> {
    plan.steps.iter().rev().find_map(|step| {
        if step.status == StepStatus::Completed {
            step.output.clone()
        } else {
            None
        }
    })
}

fn render_plan_failure(plan: &Plan, report: &ExecutionReport) -> String {
    let mut lines = vec![format!(
        "Plan execution incomplete (completed={}, failed={}, skipped={}).",
        report.completed_steps.len(),
        report.failed_steps.len(),
        report.skipped_steps.len()
    )];

    for step in &plan.steps {
        if step.status == StepStatus::Failed {
            let error = step.error.as_deref().unwrap_or("unknown failure");
            lines.push(format!("Failed step {}: {}", step.id, error));
        }
    }

    lines.join("\n")
}

async fn try_execute_with_planner(
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    model_name: &str,
    temperature: f64,
    ctx: &ExecutionContext,
) -> Option<String> {
    let tool_names = params
        .registry
        .specs_for_context(ctx)
        .into_iter()
        .map(|spec| spec.name)
        .collect::<Vec<_>>();
    let planner_request = build_planner_request(user_message, &tool_names);

    let planner_raw = match params
        .answer_provider
        .chat_with_system(
            Some(params.system_prompt),
            &planner_request,
            model_name,
            temperature,
        )
        .await
    {
        Ok(raw) => raw,
        Err(error) => {
            tracing::warn!(error = %error, "planner generation failed; falling back to direct tool loop");
            return None;
        }
    };

    let Some(plan_json) = PlanParser::extract_json(&planner_raw) else {
        tracing::warn!("planner returned no JSON; falling back to direct tool loop");
        return None;
    };

    let mut plan = match PlanParser::parse(plan_json) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(error = %error, "planner JSON parse failed; falling back to direct tool loop");
            return None;
        }
    };

    if plan.steps.len() < 3 {
        tracing::info!(
            steps = plan.steps.len(),
            "planner produced short plan; using direct tool loop"
        );
        return None;
    }

    let runner = ToolStepRunner::new(Arc::clone(&params.registry), ctx.clone());
    let report = match PlanExecutor::execute(&mut plan, &runner).await {
        Ok(report) => report,
        Err(error) => {
            tracing::warn!(error = %error, "plan execution failed; falling back to direct tool loop");
            return None;
        }
    };

    if report.success {
        final_step_output(&plan).or_else(|| Some("Plan completed.".to_string()))
    } else {
        Some(render_plan_failure(&plan, &report))
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_turn_with_plan_or_tool_loop(
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    enriched: &str,
    clamped_temperature: f64,
    ctx: &ExecutionContext,
    conversation_history: &[ProviderMessage],
    stream_sink: Option<Arc<dyn StreamSink>>,
    hooks: &[Arc<dyn PromptHook>],
) -> Result<ToolLoopResult> {
    let planner_response = if should_attempt_planner(user_message) {
        try_execute_with_planner(
            params,
            enriched,
            params.model_name,
            clamped_temperature,
            ctx,
        )
        .await
    } else {
        None
    };

    if let Some(planned_response) = planner_response {
        tracing::info!(entity_id = %ctx.entity_id, "planner path selected for main session turn");
        return Ok(ToolLoopResult {
            final_text: planned_response,
            tool_calls: Vec::new(),
            attachments: Vec::new(),
            iterations: 0,
            tokens_used: None,
            stop_reason: LoopStopReason::Completed,
        });
    }

    let tool_loop = ToolLoop::new(Arc::clone(&params.registry), params.max_tool_iterations);
    let tool_result = tool_loop
        .run(ToolLoopRunParams {
            provider: params.answer_provider,
            system_prompt: params.system_prompt,
            user_message: enriched,
            image_content: &[],
            model: params.model_name,
            temperature: clamped_temperature,
            ctx,
            stream_sink,
            conversation_history,
            hooks,
        })
        .await
        .context("run agent tool loop")?;
    tracing::debug!(
        entity_id = %ctx.entity_id,
        iterations = tool_result.iterations,
        stop_reason = ?tool_result.stop_reason,
        "main session tool loop completed"
    );
    handle_tool_loop_stop_reason(&tool_result.stop_reason, tool_result.iterations)?;
    Ok(tool_result)
}

async fn build_enriched_message(
    mem: &dyn Memory,
    write_context: &RuntimeMemoryWriteContext,
    user_message: &str,
) -> String {
    let context = build_context_with_policy(
        mem,
        &write_context.entity_id,
        user_message,
        write_context.policy_context.clone(),
    )
    .await
    .unwrap_or_default();
    enrich_user_message(&context, user_message)
}

pub async fn run_main_session_turn_for_integration(
    params: IntegrationTurnParams<'_>,
) -> Result<String> {
    run_main_session_turn_for_integration_with_policy(IntegrationTurnParams {
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        ..params
    })
    .await
}

pub async fn run_main_session_turn_for_integration_with_policy(
    params: IntegrationTurnParams<'_>,
) -> Result<String> {
    let IntegrationTurnParams {
        config,
        security,
        mem,
        answer_provider,
        reflect_provider,
        system_prompt,
        model_name,
        temperature,
        entity_id,
        policy_context,
        user_message,
    } = params;
    let observer: Arc<dyn Observer> = Arc::new(NoopObserver);
    let registry = super::run::init_tools(config, &mem);
    let person_id = resolve_person_id(config);
    let params = MainSessionTurnParams {
        answer_provider,
        reflect_provider,
        person_id: &person_id,
        system_prompt,
        model_name,
        temperature,
        registry,
        max_tool_iterations: config.autonomy.max_tool_loop_iterations,
        rate_limiter: Arc::new(EntityRateLimiter::new(
            config.autonomy.max_actions_per_hour,
            config.autonomy.max_actions_per_entity_per_hour,
        )),
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

pub async fn run_main_session_turn_for_runtime_with_policy(
    params: IntegrationTurnParams<'_>,
    runtime_options: IntegrationRuntimeTurnOptions<'_>,
) -> Result<ToolLoopResult> {
    let IntegrationTurnParams {
        config,
        security,
        mem,
        answer_provider,
        reflect_provider,
        system_prompt,
        model_name,
        temperature,
        entity_id,
        policy_context,
        user_message,
    } = params;
    let IntegrationRuntimeTurnOptions {
        registry,
        max_tool_iterations,
        execution_context,
        stream_sink,
        conversation_history,
        hooks,
    } = runtime_options;
    let observer: Arc<dyn Observer> = Arc::new(NoopObserver);
    let person_id = resolve_person_id(config);
    let session_params = MainSessionTurnParams {
        answer_provider,
        reflect_provider,
        person_id: &person_id,
        system_prompt,
        model_name,
        temperature,
        registry,
        max_tool_iterations,
        rate_limiter: Arc::clone(&execution_context.rate_limiter),
    };
    let runtime_options = MainSessionRuntimeOptions {
        execution_context_override: Some(execution_context),
        stream_sink,
        conversation_history,
        hooks,
    };

    execute_main_session_turn_with_policy_outcome(
        config,
        security,
        mem,
        &session_params,
        user_message,
        RuntimeMemoryWriteContext::for_entity_with_policy(entity_id, policy_context),
        &observer,
        &runtime_options,
    )
    .await
    .map(|outcome| outcome.tool_result)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_main_session_turn_with_accounting(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    write_context: &RuntimeMemoryWriteContext,
    observer: &Arc<dyn Observer>,
    runtime_options: &MainSessionRuntimeOptions<'_>,
) -> Result<TurnExecutionOutcome> {
    observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::IntentCreated);
    let mut accounting = TurnCallAccounting::for_persona_mode(config.persona.enabled_main_session);
    write_context.enforce_write_scope()?;

    save_user_message_if_enabled(config, mem.as_ref(), write_context, user_message).await;

    let enriched = build_enriched_message(mem.as_ref(), write_context, user_message).await;

    enforce_intent_policy(security, observer)?;
    accounting
        .consume_answer_call()
        .context("consume answer call budget")?;
    let effective_autonomy_level = config.autonomy.effective_autonomy_level();
    let clamped_temperature =
        clamp_temperature_for_turn(config, params.temperature, effective_autonomy_level);
    let default_ctx = build_main_session_execution_context(
        config,
        security,
        params,
        write_context,
        effective_autonomy_level,
    );
    let execution_context = runtime_options
        .execution_context_override
        .as_ref()
        .unwrap_or(&default_ctx);
    let tool_result = execute_turn_with_plan_or_tool_loop(
        params,
        user_message,
        &enriched,
        clamped_temperature,
        execution_context,
        runtime_options.conversation_history,
        runtime_options.stream_sink.clone(),
        runtime_options.hooks,
    )
    .await?;
    let response = tool_result.final_text.clone();
    run_persona_reflect_if_enabled(
        config,
        security,
        Arc::clone(&mem),
        params,
        user_message,
        &response,
        &mut accounting,
    )
    .await?;

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
        tool_result,
    })
}

fn enforce_intent_policy(security: &SecurityPolicy, observer: &Arc<dyn Observer>) -> Result<()> {
    match security.consume_action_and_cost(0) {
        Ok(()) => {
            observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::IntentPolicyAllowed);
            Ok(())
        }
        Err(error) => {
            observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::IntentPolicyDenied);
            Err(anyhow::Error::msg(error))
        }
    }
}

fn clamp_temperature_for_turn(
    config: &Config,
    requested_temperature: f64,
    effective_autonomy_level: AutonomyLevel,
) -> f64 {
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

    clamped_temperature
}

async fn run_persona_reflect_if_enabled(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
    response: &str,
    accounting: &mut TurnCallAccounting,
) -> Result<()> {
    if !config.persona.enabled_main_session {
        return Ok(());
    }

    security
        .consume_action_and_cost(0)
        .map_err(anyhow::Error::msg)
        .context("consume rate limit for persona reflect")?;
    accounting
        .consume_reflect_call()
        .context("consume reflect call budget")?;

    if let Err(error) = run_persona_reflect_writeback(
        config,
        mem,
        params.reflect_provider,
        params.model_name,
        params.person_id,
        user_message,
        response,
    )
    .await
    {
        tracing::warn!(error = %error, "persona reflect/writeback failed; answer path preserved");
    }

    Ok(())
}

async fn save_user_message_if_enabled(
    config: &Config,
    mem: &dyn Memory,
    write_context: &RuntimeMemoryWriteContext,
    user_message: &str,
) {
    if config.memory.auto_save {
        let input = MemoryEventInput::new(
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
        .with_source_kind(SourceKind::Conversation)
        .with_source_ref("agent.autosave.user_msg")
        .with_provenance(MemoryProvenance::source_reference(
            MemorySource::ExplicitUser,
            "agent.autosave.user_msg",
        ));
        if let Err(error) = enforce_agent_autosave_write_policy(&input) {
            tracing::warn!(%error, "agent autosave user message rejected by write policy");
        } else {
            let _ = mem.append_event(input).await;
        }
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
    let input = MemoryEventInput::new(
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
    .with_source_kind(SourceKind::Conversation)
    .with_source_ref("agent.autosave.assistant_resp")
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::System,
        "agent.autosave.assistant_resp",
    ));
    if let Err(error) = enforce_agent_autosave_write_policy(&input) {
        tracing::warn!(%error, "agent autosave assistant response rejected by write policy");
    } else {
        let _ = mem.append_event(input).await;
    }

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
            let mem_clone = Arc::clone(mem);
            let workspace_dir = config.workspace_dir.clone();
            tokio::spawn(async move {
                if let Err(error) =
                    memory::run_consolidation_once(mem_clone.as_ref(), &workspace_dir, &input).await
                {
                    tracing::warn!(error = %error, "background consolidation failed");
                }
            });
        }
        Err(error) => {
            tracing::warn!(error = %error, "post-turn consolidation checkpoint skipped");
        }
    }
}
