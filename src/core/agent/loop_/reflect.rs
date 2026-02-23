use crate::config::Config;
use crate::core::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
    SourceKind,
};
use crate::core::persona::state_header::StateHeader;
use crate::core::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use crate::core::providers::Provider;
use crate::security::writeback_guard::{
    ImmutableStateHeader, SelfTaskWriteback, WritebackGuardVerdict,
    enforce_persona_long_term_write_policy, validate_writeback_payload,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::sync::Arc;

const PERSONA_REFLECT_SYSTEM_PROMPT: &str = r#"You are a deterministic reflection/writeback stage.
Output must be a single strict JSON object, with no markdown and no extra text.

Required top-level shape:
{
  "state_header": {
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
    canonical_state: Option<&StateHeader>,
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
    person_id: &str,
    user_message: &str,
    answer: &str,
) -> Result<()> {
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        person_id,
    );

    let canonical_state = persistence
        .reconcile_mirror_from_backend_on_startup()
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
        schema_version: 1,
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

    let candidate = StateHeader {
        identity_principles_hash: previous_state.identity_principles_hash.clone(),
        safety_posture: previous_state.safety_posture.clone(),
        current_objective: accepted.state_header.current_objective,
        open_loops: accepted.state_header.open_loops,
        next_actions: accepted.state_header.next_actions,
        commitments: accepted.state_header.commitments,
        recent_context_summary: accepted.state_header.recent_context_summary,
        last_updated_at: accepted.state_header.last_updated_at,
    };

    StateHeader::validate_writeback_candidate(&previous_state, &candidate, &config.persona)
        .context("validate persona writeback candidate")?;
    persistence
        .persist_backend_canonical_and_sync_mirror(&candidate)
        .await
        .context("persist canonical persona state")?;

    for (idx, entry) in accepted.memory_append.iter().enumerate() {
        let input = MemoryEventInput::new(
            format!("person:{person_id}"),
            format!("persona.writeback.{idx}"),
            MemoryEventType::SummaryCompacted,
            entry.clone(),
            MemorySource::System,
            PrivacyLevel::Private,
        )
        .with_confidence(0.9)
        .with_importance(0.8)
        .with_source_kind(SourceKind::Manual)
        .with_source_ref(format!("persona-reflect-memory-append:{idx}"))
        .with_provenance(MemoryProvenance::source_reference(
            MemorySource::System,
            "persona.reflect.memory_append",
        ))
        .with_occurred_at(candidate.last_updated_at.clone());
        enforce_persona_long_term_write_policy(&input, person_id)
            .context("enforce persona writeback policy")?;
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
            &build_self_task_plan_command(task),
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

fn build_self_task_plan_command(task: &SelfTaskWriteback) -> String {
    let plan = json!({
        "id": "persona-self-task-plan",
        "description": "Execute persona-generated self task through planner path",
        "steps": [
            {
                "id": "step_1",
                "description": "record self task intent",
                "action": {
                    "kind": "prompt",
                    "text": format!("self-task title={} instructions={}", task.title, task.instructions)
                },
                "depends_on": []
            },
            {
                "id": "step_2",
                "description": "self task checkpoint",
                "action": {
                    "kind": "checkpoint",
                    "label": "persona-self-task-queued"
                },
                "depends_on": ["step_1"]
            }
        ]
    });
    format!("plan:{plan}")
}

#[cfg(test)]
mod tests {
    use super::build_self_task_plan_command;
    use crate::core::planner::PlanParser;
    use crate::core::planner::StepAction;
    use crate::security::writeback_guard::SelfTaskWriteback;

    #[test]
    fn self_task_plan_command_builds_valid_planner_payload() {
        let task = SelfTaskWriteback {
            title: "Follow up".to_string(),
            instructions: "Inspect outstanding regressions".to_string(),
            expires_at: "2026-02-28T12:00:00Z".to_string(),
        };

        let command = build_self_task_plan_command(&task);
        assert!(command.starts_with("plan:"));

        let payload = command.strip_prefix("plan:").expect("plan prefix");
        let plan = PlanParser::parse(payload).expect("valid plan payload");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].id, "step_1");
        assert_eq!(plan.steps[1].id, "step_2");
        assert_eq!(plan.steps[1].depends_on, vec!["step_1"]);
        assert!(matches!(plan.steps[0].action, StepAction::Prompt { .. }));
        match &plan.steps[1].action {
            StepAction::Checkpoint { label } => {
                assert_eq!(label, "persona-self-task-queued");
            }
            StepAction::Prompt { .. } | StepAction::ToolCall { .. } => {
                panic!("expected checkpoint action")
            }
        }
    }

    #[test]
    fn self_task_plan_command_escapes_special_characters_in_prompt_text() {
        let task = SelfTaskWriteback {
            title: "Follow \"critical\" task".to_string(),
            instructions: "Inspect line1\nline2 and preserve \"quotes\"".to_string(),
            expires_at: "2026-02-28T12:00:00Z".to_string(),
        };

        let command = build_self_task_plan_command(&task);
        let payload = command.strip_prefix("plan:").expect("plan prefix");
        let plan = PlanParser::parse(payload).expect("valid escaped plan payload");

        match &plan.steps[0].action {
            StepAction::Prompt { text } => {
                assert!(text.contains("critical"));
                assert!(text.contains("line1"));
                assert!(text.contains("line2"));
                assert!(text.contains("quotes"));
            }
            StepAction::ToolCall { .. } | StepAction::Checkpoint { .. } => {
                panic!("expected prompt action")
            }
        }
    }
}
