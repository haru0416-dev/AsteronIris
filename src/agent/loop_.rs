use crate::auth::AuthBroker;
use crate::config::Config;
use crate::memory::traits::MemoryLayer;
use crate::memory::{
    self, Memory, MemoryEventInput, MemoryEventType, MemoryInferenceEvent, MemoryProvenance,
    MemoryRecallItem, MemorySource, PrivacyLevel, RecallQuery,
};
use crate::observability::{self, Observer, ObserverEvent};
use crate::persona::state_header::StateHeaderV1;
use crate::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::external_content::{
    decide_external_action, detect_injection_signals, sanitize_marker_collision,
    wrap_external_content, ExternalAction,
};
use crate::security::policy::TenantPolicyContext;
use crate::security::writeback_guard::{
    validate_writeback_payload, ImmutableStateHeader, SelfTaskWriteback, WritebackGuardVerdict,
};
use crate::security::SecurityPolicy;
use crate::tools;
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifyRepairCaps {
    max_attempts: u32,
    max_repair_depth: u32,
}

impl VerifyRepairCaps {
    fn from_config(config: &Config) -> Self {
        Self {
            max_attempts: config.autonomy.verify_repair_max_attempts,
            max_repair_depth: config.autonomy.verify_repair_max_repair_depth,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerifyRepairEscalationReason {
    MaxAttemptsReached,
    MaxRepairDepthReached,
    NonRetryableFailure,
}

impl VerifyRepairEscalationReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::MaxAttemptsReached => "max_attempts_reached",
            Self::MaxRepairDepthReached => "max_repair_depth_reached",
            Self::NonRetryableFailure => "non_retryable_failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifyFailureAnalysis {
    failure_class: &'static str,
    retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifyRepairEscalation {
    reason: VerifyRepairEscalationReason,
    attempts: u32,
    repair_depth: u32,
    max_attempts: u32,
    max_repair_depth: u32,
    failure_class: &'static str,
    last_error: String,
}

impl VerifyRepairEscalation {
    fn contract_message(&self) -> String {
        format!(
            "verify/repair escalated: reason={} attempts={} repair_depth={} max_attempts={} max_repair_depth={} failure_class={} last_error={}",
            self.reason.as_str(),
            self.attempts,
            self.repair_depth,
            self.max_attempts,
            self.max_repair_depth,
            self.failure_class,
            self.last_error
        )
    }

    fn event_payload(&self) -> Value {
        json!({
            "reason": self.reason.as_str(),
            "attempts": self.attempts,
            "repair_depth": self.repair_depth,
            "max_attempts": self.max_attempts,
            "max_repair_depth": self.max_repair_depth,
            "failure_class": self.failure_class,
            "last_error": self.last_error,
        })
    }
}

const VERIFY_REPAIR_ESCALATION_SLOT_KEY: &str = "autonomy.verify_repair.escalation";

fn analyze_verify_failure(error: &anyhow::Error) -> VerifyFailureAnalysis {
    let message = error.to_string();
    if message.contains("action limit exceeded") || message.contains("daily cost limit exceeded") {
        return VerifyFailureAnalysis {
            failure_class: "policy_limit",
            retryable: false,
        };
    }

    VerifyFailureAnalysis {
        failure_class: "transient_failure",
        retryable: true,
    }
}

fn decide_verify_repair_escalation(
    caps: VerifyRepairCaps,
    attempts: u32,
    repair_depth: u32,
    analysis: VerifyFailureAnalysis,
    last_error: &anyhow::Error,
) -> Option<VerifyRepairEscalation> {
    let reason = if attempts >= caps.max_attempts {
        Some(VerifyRepairEscalationReason::MaxAttemptsReached)
    } else if repair_depth >= caps.max_repair_depth {
        Some(VerifyRepairEscalationReason::MaxRepairDepthReached)
    } else if !analysis.retryable {
        Some(VerifyRepairEscalationReason::NonRetryableFailure)
    } else {
        None
    }?;

    Some(VerifyRepairEscalation {
        reason,
        attempts,
        repair_depth,
        max_attempts: caps.max_attempts,
        max_repair_depth: caps.max_repair_depth,
        failure_class: analysis.failure_class,
        last_error: last_error.to_string(),
    })
}

async fn emit_verify_repair_escalation_event(
    mem: &dyn Memory,
    escalation: &VerifyRepairEscalation,
) -> Result<()> {
    let event = MemoryEventInput::new(
        "default",
        VERIFY_REPAIR_ESCALATION_SLOT_KEY,
        MemoryEventType::SummaryCompacted,
        escalation.event_payload().to_string(),
        MemorySource::System,
        PrivacyLevel::Private,
    )
    .with_confidence(1.0)
    .with_importance(0.9);
    mem.append_event(event).await?;
    Ok(())
}

struct MainSessionTurnParams<'a> {
    answer_provider: &'a dyn Provider,
    reflect_provider: &'a dyn Provider,
    system_prompt: &'a str,
    model_name: &'a str,
    temperature: f64,
}

#[derive(Debug, Clone)]
struct RuntimeMemoryWriteContext {
    entity_id: String,
    policy_context: TenantPolicyContext,
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

    fn enforce_write_scope(&self) -> Result<()> {
        self.policy_context
            .enforce_recall_scope(&self.entity_id)
            .map_err(anyhow::Error::msg)
    }
}

/// Build context preamble by searching memory for relevant entries
fn sanitize_external_fragment_for_context(slot_key: &str, value: &str) -> String {
    if !value.contains("digest_sha256=") {
        return "[external payload omitted by replay-ban policy]".to_string();
    }

    let signals = detect_injection_signals(value);
    let action = decide_external_action(&signals);
    match action {
        ExternalAction::Allow => wrap_external_content(slot_key, value),
        ExternalAction::Sanitize => {
            let sanitized = sanitize_marker_collision(value);
            wrap_external_content(slot_key, &sanitized)
        }
        ExternalAction::Block => {
            "[external summary blocked by policy during context replay]".to_string()
        }
    }
}

const CONTEXT_REPLAY_REVOKED_MARKERS: [&str; 2] = [
    "__LANCEDB_DEGRADED_SOFT_FORGET_MARKER__",
    "__LANCEDB_DEGRADED_TOMBSTONE_MARKER__",
];

fn is_revocation_marker_payload(value: &str) -> bool {
    CONTEXT_REPLAY_REVOKED_MARKERS
        .iter()
        .any(|marker| value.contains(marker))
}

async fn allow_context_replay_item(mem: &dyn Memory, entry: &MemoryRecallItem) -> bool {
    if is_revocation_marker_payload(&entry.value) {
        return false;
    }

    let resolved = mem.resolve_slot(&entry.entity_id, &entry.slot_key).await;
    matches!(resolved, Ok(Some(slot)) if slot.value == entry.value)
}

async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    build_context_with_policy(mem, "default", user_msg, TenantPolicyContext::disabled())
        .await
        .unwrap_or_default()
}

fn build_context_recall_query(
    entity_id: &str,
    user_msg: &str,
    policy_context: TenantPolicyContext,
) -> Result<RecallQuery> {
    let query = RecallQuery::new(entity_id, user_msg, 5).with_policy_context(policy_context);
    query.enforce_policy()?;
    Ok(query)
}

async fn build_context_with_policy(
    mem: &dyn Memory,
    entity_id: &str,
    user_msg: &str,
    policy_context: TenantPolicyContext,
) -> Result<String> {
    let mut context = String::new();

    // Pull relevant memories for this message
    let query = build_context_recall_query(entity_id, user_msg, policy_context)?;
    let entries = mem.recall_scoped(query).await?;
    let mut replayable_entries = Vec::with_capacity(entries.len());
    for entry in entries {
        if allow_context_replay_item(mem, &entry).await {
            replayable_entries.push(entry);
        }
    }

    if !replayable_entries.is_empty() {
        context.push_str("[Memory context]\n");
        for entry in &replayable_entries {
            let value = if entry.slot_key.starts_with("external.") {
                sanitize_external_fragment_for_context(&entry.slot_key, &entry.value)
            } else {
                entry.value.clone()
            };
            let _ = writeln!(context, "- {}: {}", entry.slot_key, value);
        }
        context.push('\n');
    }

    Ok(context)
}

pub async fn build_context_for_integration(
    mem: &dyn Memory,
    entity_id: &str,
    user_msg: &str,
    policy_context: TenantPolicyContext,
) -> Result<String> {
    build_context_with_policy(mem, entity_id, user_msg, policy_context).await
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

    enqueue_reflect_self_tasks(config, &accepted.self_tasks);

    Ok(())
}

fn enqueue_reflect_self_tasks(config: &Config, self_tasks: &[SelfTaskWriteback]) {
    for task in self_tasks {
        let parsed_expires_at = match DateTime::parse_from_rfc3339(&task.expires_at) {
            Ok(value) => value.with_timezone(&Utc),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    expires_at = %task.expires_at,
                    "skipping self task enqueue due to invalid expires_at"
                );
                continue;
            }
        };

        let metadata = crate::cron::CronJobMetadata {
            job_kind: crate::cron::CronJobKind::Agent,
            origin: crate::cron::CronJobOrigin::Agent,
            expires_at: Some(parsed_expires_at),
            max_attempts: config.autonomy.verify_repair_max_attempts.max(1),
        };

        if let Err(error) = crate::cron::add_job_with_metadata(
            config,
            "* * * * *",
            "echo agent-self-task",
            &metadata,
        ) {
            tracing::warn!(
                error = %error,
                title = %task.title,
                "failed to enqueue reflect self task"
            );
        }
    }
}

async fn execute_main_session_turn(
    config: &Config,
    security: &SecurityPolicy,
    mem: Arc<dyn Memory>,
    params: &MainSessionTurnParams<'_>,
    user_message: &str,
) -> Result<String> {
    execute_main_session_turn_with_policy(
        config,
        security,
        mem,
        params,
        user_message,
        RuntimeMemoryWriteContext::main_session_default(),
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
) -> Result<TurnExecutionOutcome> {
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

    security
        .consume_action_and_cost(0)
        .map_err(anyhow::Error::msg)?;
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
            run_post_turn_inference_pass(mem.as_ref(), write_context, &response).await
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

fn parse_inference_payload(line: &str) -> Option<(&str, &str)> {
    let (slot_key, value) = line.split_once("=>")?;
    let slot_key = slot_key.trim();
    let value = value.trim();
    if slot_key.is_empty() || value.is_empty() {
        return None;
    }
    Some((slot_key, value))
}

fn build_post_turn_inference_events(
    entity_id: &str,
    assistant_response: &str,
) -> Vec<MemoryInferenceEvent> {
    const INFERRED_PREFIX: &str = "INFERRED_CLAIM ";
    const CONTRADICTION_PREFIX: &str = "CONTRADICTION_EVENT ";

    assistant_response
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(payload) = line.strip_prefix(INFERRED_PREFIX) {
                let (slot_key, value) = parse_inference_payload(payload)?;
                return Some(
                    MemoryInferenceEvent::inferred_claim(entity_id, slot_key, value)
                        .with_layer(MemoryLayer::Semantic),
                );
            }
            if let Some(payload) = line.strip_prefix(CONTRADICTION_PREFIX) {
                let (slot_key, value) = parse_inference_payload(payload)?;
                return Some(
                    MemoryInferenceEvent::contradiction_marked(entity_id, slot_key, value)
                        .with_layer(MemoryLayer::Episodic),
                );
            }
            None
        })
        .collect()
}

fn inference_provenance_reference(event: &MemoryInferenceEvent) -> (&'static str, MemorySource) {
    match event {
        MemoryInferenceEvent::InferredClaim { .. } => {
            ("inference.post_turn.inferred_claim", MemorySource::Inferred)
        }
        MemoryInferenceEvent::ContradictionEvent { .. } => (
            "inference.post_turn.contradiction_event",
            MemorySource::System,
        ),
    }
}

async fn run_post_turn_inference_pass(
    mem: &dyn Memory,
    write_context: &RuntimeMemoryWriteContext,
    assistant_response: &str,
) -> Result<()> {
    write_context.enforce_write_scope()?;
    let events = build_post_turn_inference_events(&write_context.entity_id, assistant_response);
    if events.is_empty() {
        return Ok(());
    }

    for event in events {
        let (reference, source_class) = inference_provenance_reference(&event);
        let input = event
            .into_memory_event_input()
            .with_provenance(MemoryProvenance::source_reference(source_class, reference));
        mem.append_event(input).await?;
    }

    Ok(())
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
        let response =
            execute_main_session_turn(&config, security.as_ref(), mem.clone(), &turn_params, &msg)
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
    use crate::providers::reliable::ReliableProvider;
    use crate::security::SecurityPolicy;
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
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
        )
        .await
        .unwrap();

        assert_eq!(response, "clamped-temp-response");
        let seen = temperatures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(seen, vec![1.2]);
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
        )
        .await
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("reason=non_retryable_failure"));
        assert!(message.contains("failure_class=policy_limit"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn build_context_replay_ban_hides_raw_external_payload() {
        let temp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "external.gateway.webhook",
                MemoryEventType::FactAdded,
                "ATTACK_PAYLOAD_ALPHA",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.7),
        )
        .await
        .unwrap();

        let context = build_context(mem.as_ref(), "ATTACK_PAYLOAD_ALPHA").await;
        assert!(context.contains("external.gateway.webhook"));
        assert!(!context.contains("ATTACK_PAYLOAD_ALPHA"));
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
