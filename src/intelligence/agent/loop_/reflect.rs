use crate::config::Config;
use crate::intelligence::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel,
};
use crate::intelligence::providers::Provider;
use crate::persona::state_header::StateHeaderV1;
use crate::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use crate::security::writeback_guard::{
    ImmutableStateHeader, SelfTaskWriteback, WritebackGuardVerdict, validate_writeback_payload,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

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
        Some(state) => {
            serde_json::to_string_pretty(state).context("serialize canonical state header")?
        }
        None => "null".to_string(),
    };

    Ok(format!(
        "Current canonical state header (JSON):\n{canonical_json}\n\nLatest user message:\n{user_message}\n\nLatest assistant answer:\n{answer}\n\nReturn only the strict JSON payload."
    ))
}

fn parse_reflect_payload(raw: &str) -> Result<Value> {
    let payload: Value = serde_json::from_str(raw.trim()).context("parse reflect payload JSON")?;
    if !payload.is_object() {
        anyhow::bail!("reflect output must be a JSON object");
    }
    Ok(payload)
}

pub(super) async fn run_persona_reflect_writeback(
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

    let canonical_state = persistence
        .load_backend_canonical()
        .await
        .context("load canonical persona state")?;
    let reflect_message = build_reflect_message(canonical_state.as_ref(), user_message, answer)
        .context("build persona reflect message")?;

    let reflect_raw = provider
        .chat_with_system(
            Some(PERSONA_REFLECT_SYSTEM_PROMPT),
            &reflect_message,
            model_name,
            0.0,
        )
        .await
        .context("call reflect provider for persona writeback")?;
    let reflect_payload =
        parse_reflect_payload(&reflect_raw).context("parse persona reflect payload")?;

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

    StateHeaderV1::validate_writeback_candidate(&previous_state, &candidate, &config.persona)
        .context("validate persona writeback candidate")?;
    persistence
        .persist_backend_canonical_and_sync_mirror(&candidate)
        .await
        .context("persist canonical persona state")?;

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
        mem.append_event(input)
            .await
            .context("append persona writeback memory event")?;
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

        let metadata = crate::platform::cron::CronJobMetadata {
            job_kind: crate::platform::cron::CronJobKind::Agent,
            origin: crate::platform::cron::CronJobOrigin::Agent,
            expires_at: Some(parsed_expires_at),
            max_attempts: config.autonomy.verify_repair_max_attempts.max(1),
        };

        if let Err(error) = crate::platform::cron::add_job_with_metadata(
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
