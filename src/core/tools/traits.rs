use crate::core::tools::middleware::ExecutionContext;
use crate::security::{ActionPolicyVerdict, ExternalActionExecution, SecurityPolicy};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

/// Result of a tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
    #[serde(default)]
    pub attachments: Vec<OutputAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputAttachment {
    pub mime_type: String,
    pub filename: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
}

impl OutputAttachment {
    pub fn from_path(
        mime_type: impl Into<String>,
        path: impl Into<String>,
        filename: Option<String>,
    ) -> Self {
        Self {
            mime_type: mime_type.into(),
            filename,
            path: Some(path.into()),
            url: None,
        }
    }

    pub fn from_url(
        mime_type: impl Into<String>,
        url: impl Into<String>,
        filename: Option<String>,
    ) -> Self {
        Self {
            mime_type: mime_type.into(),
            filename,
            path: None,
            url: Some(url.into()),
        }
    }
}

/// Description of a tool for the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Core tool trait — implement for any capability
pub trait Tool: Send + Sync {
    /// Tool name (used in LLM function calling)
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON schema for parameters
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with given arguments
    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>>;

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

pub trait ActionOperator: Send + Sync {
    fn name(&self) -> &str;

    fn apply<'a>(
        &'a self,
        intent: &'a ActionIntent,
        verdict: Option<&'a ActionPolicyVerdict>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ActionResult>> + Send + 'a>>;
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
        Ok(path.to_string_lossy().into_owned())
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
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ActionResult>> + Send + 'a>> {
        Box::pin(async move {
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputAttachment, ToolResult};
    use serde_json::json;

    #[test]
    fn tool_result_serde_defaults_attachments_when_missing() {
        let raw = json!({
            "success": true,
            "output": "ok",
            "error": null
        });

        let parsed: ToolResult = serde_json::from_value(raw).unwrap();
        assert!(parsed.attachments.is_empty());
    }

    #[test]
    fn tool_result_serde_roundtrip_with_empty_attachments() {
        let result = ToolResult {
            success: true,
            output: "ok".to_string(),
            error: None,
            attachments: Vec::new(),
        };

        let json = serde_json::to_value(&result).unwrap();
        let parsed: ToolResult = serde_json::from_value(json).unwrap();
        assert!(parsed.success);
        assert!(parsed.attachments.is_empty());
    }

    #[test]
    fn tool_result_serde_roundtrip_with_attachments() {
        let result = ToolResult {
            success: true,
            output: "done".to_string(),
            error: None,
            attachments: vec![OutputAttachment::from_path(
                "image/png",
                "/tmp/chart.png",
                Some("chart.png".to_string()),
            )],
        };

        let json = serde_json::to_value(&result).unwrap();
        let parsed: ToolResult = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(
            parsed.attachments[0].path.as_deref(),
            Some("/tmp/chart.png")
        );
    }

    #[test]
    fn output_attachment_from_path_sets_path_only() {
        let attachment = OutputAttachment::from_path(
            "image/png",
            "/tmp/image.png",
            Some("image.png".to_string()),
        );

        assert_eq!(attachment.mime_type, "image/png");
        assert_eq!(attachment.path.as_deref(), Some("/tmp/image.png"));
        assert!(attachment.url.is_none());
    }

    #[test]
    fn output_attachment_from_url_sets_url_only() {
        let attachment = OutputAttachment::from_url(
            "image/png",
            "https://example.com/image.png",
            Some("image.png".to_string()),
        );

        assert_eq!(attachment.mime_type, "image/png");
        assert!(attachment.path.is_none());
        assert_eq!(
            attachment.url.as_deref(),
            Some("https://example.com/image.png")
        );
    }

    #[test]
    fn output_attachment_serde_roundtrip_path_variant() {
        let attachment =
            OutputAttachment::from_path("text/plain", "/tmp/out.txt", Some("out.txt".to_string()));

        let json = serde_json::to_value(&attachment).unwrap();
        let parsed: OutputAttachment = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.path.as_deref(), Some("/tmp/out.txt"));
    }

    #[test]
    fn output_attachment_serde_roundtrip_url_variant() {
        let attachment =
            OutputAttachment::from_url("application/pdf", "https://example.com/report.pdf", None);

        let json = serde_json::to_value(&attachment).unwrap();
        let parsed: OutputAttachment = serde_json::from_value(json).unwrap();
        assert_eq!(
            parsed.url.as_deref(),
            Some("https://example.com/report.pdf")
        );
    }
}
