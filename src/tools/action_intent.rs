use crate::security::policy::{ActionPolicyVerdict, ExternalActionExecution, SecurityPolicy};
use anyhow::{Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionIntent {
    pub intent_id: String,
    pub action_kind: String,
    pub operator: String,
    pub payload: serde_json::Value,
    pub requested_at: String,
}

impl ActionIntent {
    pub fn new(
        action_kind: impl Into<String>,
        operator: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            intent_id: uuid::Uuid::new_v4().to_string(),
            action_kind: action_kind.into(),
            operator: operator.into(),
            payload,
            requested_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn policy_verdict(&self, security: &SecurityPolicy) -> ActionPolicyVerdict {
        match security.external_action_execution {
            ExternalActionExecution::Enabled => {
                ActionPolicyVerdict::allow("external_action_execution is enabled")
            }
            ExternalActionExecution::Disabled => {
                ActionPolicyVerdict::deny("external_action_execution is disabled")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub executed: bool,
    pub message: String,
    pub audit_record_path: Option<String>,
}

pub trait ActionOperator: Send + Sync {
    fn name(&self) -> &str;

    fn apply<'a>(
        &'a self,
        intent: &'a ActionIntent,
        verdict: Option<&'a ActionPolicyVerdict>,
    ) -> Pin<Box<dyn Future<Output = Result<ActionResult>> + Send + 'a>>;
}

#[derive(Debug)]
pub struct NoopOperator {
    security: Arc<SecurityPolicy>,
}

impl NoopOperator {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }
}

impl ActionOperator for NoopOperator {
    fn name(&self) -> &str {
        "noop"
    }

    fn apply<'a>(
        &'a self,
        intent: &'a ActionIntent,
        verdict: Option<&'a ActionPolicyVerdict>,
    ) -> Pin<Box<dyn Future<Output = Result<ActionResult>> + Send + 'a>> {
        Box::pin(async move {
            let Some(verdict) = verdict else {
                bail!("policy verdict required before applying action intent");
            };

            let audit_dir = self.security.workspace_dir.join("action_intents");
            std::fs::create_dir_all(&audit_dir)?;
            let audit_path = audit_dir.join(format!("{}.jsonl", intent.intent_id));

            let record = serde_json::json!({
                "intent_id": intent.intent_id,
                "action_kind": intent.action_kind,
                "operator": "noop",
                "verdict_allowed": verdict.allowed,
                "verdict_reason": verdict.reason,
                "executed": false,
                "timestamp": Utc::now().to_rfc3339(),
            });
            std::fs::write(&audit_path, serde_json::to_string(&record)?)?;

            Ok(ActionResult {
                executed: false,
                message: format!("noop operator: {} ({})", verdict.reason, intent.action_kind),
                audit_record_path: Some(audit_path.to_string_lossy().to_string()),
            })
        })
    }
}
