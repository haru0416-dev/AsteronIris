use crate::security::{ActionPolicyVerdict, ExternalActionExecution, SecurityPolicy};
use crate::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// Description of a tool for the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Core tool trait — implement for any capability
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (used in LLM function calling)
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON schema for parameters
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with given arguments
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult>;

    /// Get the full spec for LLM registration
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionIntent {
    pub intent_id: String,
    pub action_kind: String,
    pub operator: String,
    pub payload: serde_json::Value,
    pub requested_at: String,
}

impl ActionIntent {
    pub fn new(action_kind: &str, operator: &str, payload: serde_json::Value) -> Self {
        Self {
            intent_id: uuid::Uuid::new_v4().to_string(),
            action_kind: action_kind.to_string(),
            operator: operator.to_string(),
            payload,
            requested_at: Utc::now().to_rfc3339(),
        }
    }

    #[allow(clippy::unused_self)]
    pub fn policy_verdict(&self, policy: &SecurityPolicy) -> ActionPolicyVerdict {
        if !policy.can_act() {
            return ActionPolicyVerdict::deny("blocked by security policy: autonomy is read-only");
        }

        if policy.external_action_execution == ExternalActionExecution::Disabled {
            return ActionPolicyVerdict::deny(
                "blocked by security policy: external_action_execution is disabled",
            );
        }

        ActionPolicyVerdict::allow("allowed by security policy")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub executed: bool,
    pub message: String,
    pub audit_record_path: Option<String>,
}

#[async_trait]
pub trait ActionOperator: Send + Sync {
    fn name(&self) -> &str;

    async fn apply(
        &self,
        intent: &ActionIntent,
        verdict: Option<&ActionPolicyVerdict>,
    ) -> anyhow::Result<ActionResult>;
}

pub struct NoopOperator {
    security: Arc<SecurityPolicy>,
}

impl NoopOperator {
    pub fn new(security: Arc<SecurityPolicy>) -> Self {
        Self { security }
    }

    fn audit_path(&self) -> PathBuf {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        self.security
            .workspace_dir
            .join("action_intents")
            .join(format!("{date}.jsonl"))
    }

    async fn append_audit_record(
        &self,
        intent: &ActionIntent,
        verdict: &ActionPolicyVerdict,
        message: &str,
    ) -> anyhow::Result<String> {
        let path = self.audit_path();

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        let record = serde_json::json!({
            "recorded_at": Utc::now().to_rfc3339(),
            "operator": self.name(),
            "intent": intent,
            "policy_verdict": verdict,
            "executed": false,
            "message": message,
        });

        file.write_all(record.to_string().as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(path.to_string_lossy().to_string())
    }
}

#[async_trait]
impl ActionOperator for NoopOperator {
    fn name(&self) -> &str {
        "noop"
    }

    async fn apply(
        &self,
        intent: &ActionIntent,
        verdict: Option<&ActionPolicyVerdict>,
    ) -> anyhow::Result<ActionResult> {
        let verdict = verdict.ok_or_else(|| anyhow::anyhow!("policy verdict required"))?;

        let message = if verdict.allowed {
            "external action execution is disabled by default"
        } else {
            verdict.reason.as_str()
        };

        let audit_record_path = Some(self.append_audit_record(intent, verdict, message).await?);

        Ok(ActionResult {
            executed: false,
            message: message.to_string(),
            audit_record_path,
        })
    }
}
