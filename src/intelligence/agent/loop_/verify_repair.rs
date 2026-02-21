use crate::config::Config;
use crate::memory::{Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel};
use anyhow::Result;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VerifyRepairCaps {
    pub(super) max_attempts: u32,
    pub(super) max_repair_depth: u32,
}

impl VerifyRepairCaps {
    pub(super) fn from_config(config: &Config) -> Self {
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
pub(super) struct VerifyFailureAnalysis {
    pub(super) failure_class: &'static str,
    pub(super) retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct VerifyRepairEscalation {
    reason: VerifyRepairEscalationReason,
    pub(super) attempts: u32,
    pub(super) repair_depth: u32,
    pub(super) max_attempts: u32,
    pub(super) max_repair_depth: u32,
    pub(super) failure_class: &'static str,
    pub(super) last_error: String,
}

impl VerifyRepairEscalation {
    pub(super) fn contract_message(&self) -> String {
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

pub(super) const VERIFY_REPAIR_ESCALATION_SLOT_KEY: &str = "autonomy.verify_repair.escalation";

pub(super) fn analyze_verify_failure(error: &anyhow::Error) -> VerifyFailureAnalysis {
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

pub(super) fn decide_verify_repair_escalation(
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

pub(super) async fn emit_verify_repair_escalation_event(
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
