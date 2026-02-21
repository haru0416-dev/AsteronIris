use crate::intelligence::providers::scrub_secret_patterns;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub intent_id: String,
    pub tool_name: String,
    pub args_summary: String,
    pub risk_level: RiskLevel,
    pub entity_id: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
    ApprovedWithGrant(PermissionGrant),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub tool: String,
    pub pattern: String,
    pub scope: GrantScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    Session,
    Permanent,
}

#[async_trait]
pub trait ApprovalBroker: Send + Sync {
    async fn request_approval(&self, request: &ApprovalRequest)
    -> anyhow::Result<ApprovalDecision>;
}

pub struct AutoDenyBroker {
    pub reason: String,
}

#[async_trait]
impl ApprovalBroker for AutoDenyBroker {
    async fn request_approval(
        &self,
        _request: &ApprovalRequest,
    ) -> anyhow::Result<ApprovalDecision> {
        Ok(ApprovalDecision::Denied {
            reason: self.reason.clone(),
        })
    }
}

#[must_use]
pub fn classify_risk(tool_name: &str) -> RiskLevel {
    match tool_name {
        "shell" => RiskLevel::High,
        "file_write" | "memory_forget" | "memory_governance" => RiskLevel::Medium,
        _ => RiskLevel::Low,
    }
}

#[must_use]
pub fn summarize_args(tool_name: &str, args: &serde_json::Value) -> String {
    let raw = match tool_name {
        "shell" => args
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("(unknown)")
            .to_string(),
        "file_write" => {
            let path = args
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let len = args
                .get("content")
                .and_then(serde_json::Value::as_str)
                .map_or(0, str::len);
            format!("write {len} bytes to {path}")
        }
        "file_read" => args
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("?")
            .to_string(),
        _ => serde_json::to_string(args).unwrap_or_default(),
    };

    scrub_secret_patterns(&raw).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        ApprovalBroker, ApprovalDecision, ApprovalRequest, AutoDenyBroker, GrantScope,
        PermissionGrant, RiskLevel, classify_risk, summarize_args,
    };

    #[test]
    fn classify_risk_shell_is_high() {
        assert_eq!(classify_risk("shell"), RiskLevel::High);
    }

    #[test]
    fn classify_risk_file_read_is_low() {
        assert_eq!(classify_risk("file_read"), RiskLevel::Low);
    }

    #[test]
    fn classify_risk_file_write_is_medium() {
        assert_eq!(classify_risk("file_write"), RiskLevel::Medium);
    }

    #[tokio::test]
    async fn auto_deny_broker_denies_all_requests() {
        let broker = AutoDenyBroker {
            reason: "non-interactive context".to_string(),
        };
        let request = ApprovalRequest {
            intent_id: "intent-1".to_string(),
            tool_name: "shell".to_string(),
            args_summary: "ls".to_string(),
            risk_level: RiskLevel::High,
            entity_id: "entity-1".to_string(),
            channel: "email".to_string(),
        };

        let decision = broker
            .request_approval(&request)
            .await
            .expect("auto deny broker should not fail");

        assert_eq!(
            decision,
            ApprovalDecision::Denied {
                reason: "non-interactive context".to_string()
            }
        );
    }

    #[test]
    fn permission_grant_round_trip_serde() {
        let grant = PermissionGrant {
            tool: "file_write".to_string(),
            pattern: "notes/*.md".to_string(),
            scope: GrantScope::Session,
        };

        let json = serde_json::to_string(&grant).expect("serialize grant");
        let decoded: PermissionGrant = serde_json::from_str(&json).expect("deserialize grant");

        assert_eq!(grant, decoded);
    }

    #[test]
    fn summarize_args_shell_command() {
        let summary = summarize_args("shell", &serde_json::json!({ "command": "ls" }));
        assert_eq!(summary, "ls");
    }

    #[test]
    fn summarize_args_file_write_details() {
        let summary = summarize_args(
            "file_write",
            &serde_json::json!({ "path": "foo.txt", "content": "hello" }),
        );
        assert_eq!(summary, "write 5 bytes to foo.txt");
    }

    #[test]
    fn approval_decision_approved_equality() {
        assert_eq!(ApprovalDecision::Approved, ApprovalDecision::Approved);
    }
}
