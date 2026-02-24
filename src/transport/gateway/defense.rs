use crate::config::GatewayDefenseMode;
use crate::security::external_content::{ExternalAction, prepare_external_content};
use axum::{http::StatusCode, response::Json};

use super::AppState;

#[derive(Debug, Clone)]
pub(super) struct ExternalIngressPolicyOutcome {
    pub(super) model_input: String,
    pub(super) persisted_summary: String,
    pub(super) blocked: bool,
}

/// Apply external ingress policy to incoming content.
pub(super) fn apply_external_ingress_policy(
    source: &str,
    text: &str,
) -> ExternalIngressPolicyOutcome {
    let prepared = prepare_external_content(source, text);
    let blocked = prepared.action == ExternalAction::Block;
    let persisted_summary = prepared.persisted_summary.as_memory_value();
    let model_input = prepared.model_input;

    ExternalIngressPolicyOutcome {
        model_input,
        persisted_summary,
        blocked,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PolicyViolation {
    MissingOrInvalidBearer,
    MissingOrInvalidWebhookSecret,
    NoAuthConfigured,
}

impl PolicyViolation {
    pub(super) fn is_auth_violation(self) -> bool {
        matches!(
            self,
            Self::MissingOrInvalidBearer | Self::MissingOrInvalidWebhookSecret
        )
    }

    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::MissingOrInvalidBearer => "missing_or_invalid_bearer",
            Self::MissingOrInvalidWebhookSecret => "missing_or_invalid_webhook_secret",
            Self::NoAuthConfigured => "no_auth_configured",
        }
    }

    pub(super) fn enforce_response(self) -> (StatusCode, Json<serde_json::Value>) {
        match self {
            Self::MissingOrInvalidBearer => {
                let err = serde_json::json!({
                    "error": "Unauthorized — pair first via POST /pair, then send Authorization: Bearer <token>"
                });
                (StatusCode::UNAUTHORIZED, Json(err))
            }
            Self::MissingOrInvalidWebhookSecret => {
                let err = serde_json::json!({"error": "Unauthorized — invalid or missing X-Webhook-Secret header"});
                (StatusCode::UNAUTHORIZED, Json(err))
            }
            Self::NoAuthConfigured => {
                let err = serde_json::json!({
                    "error": "Forbidden — no authentication configured. Pair first via POST /pair or configure a webhook secret."
                });
                (StatusCode::FORBIDDEN, Json(err))
            }
        }
    }
}

pub(super) fn effective_defense_mode(state: &AppState) -> GatewayDefenseMode {
    if state.defense_kill_switch {
        GatewayDefenseMode::Audit
    } else {
        state.defense_mode
    }
}

pub(super) fn policy_violation_response(
    state: &AppState,
    violation: PolicyViolation,
) -> Option<(StatusCode, Json<serde_json::Value>)> {
    debug_assert!(
        !violation.is_auth_violation(),
        "auth violations must be handled by must_enforce_auth_violation"
    );

    let mode = effective_defense_mode(state);
    let reason = violation.reason();
    match mode {
        GatewayDefenseMode::Audit => {
            tracing::warn!(
                mode = "audit",
                violation = reason,
                "Webhook policy violation recorded"
            );
            None
        }
        GatewayDefenseMode::Warn => {
            tracing::warn!(
                mode = "warn",
                violation = reason,
                "Webhook policy violation warning"
            );
            Some((
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "mode": "warn",
                    "warning": reason,
                    "blocked": false
                })),
            ))
        }
        GatewayDefenseMode::Enforce => {
            tracing::warn!(
                mode = "enforce",
                violation = reason,
                "Webhook policy violation blocked"
            );
            Some(violation.enforce_response())
        }
    }
}

pub(super) fn must_enforce_auth_violation(state: &AppState, violation: PolicyViolation) -> bool {
    debug_assert!(violation.is_auth_violation());

    let mode = effective_defense_mode(state);
    let reason = violation.reason();

    match mode {
        GatewayDefenseMode::Audit => {
            tracing::warn!(
                mode = "audit",
                violation = reason,
                "Authentication violation blocked (audit mode logging)"
            );
        }
        GatewayDefenseMode::Warn => {
            tracing::warn!(
                mode = "warn",
                violation = reason,
                "Authentication violation blocked (warn mode logging)"
            );
        }
        GatewayDefenseMode::Enforce => {
            tracing::warn!(
                mode = "enforce",
                violation = reason,
                "Authentication violation blocked"
            );
        }
    }

    true
}

pub(super) fn policy_accounting_response(
    policy_error: &'static str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({"error": policy_error})),
    )
}
