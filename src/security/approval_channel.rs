use crate::security::approval::{
    ApprovalBroker, ApprovalDecision, ApprovalRequest, AutoDenyBroker,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub struct TextReplyApprovalBroker {
    pub channel_name: String,
    pub timeout: Duration,
}

impl TextReplyApprovalBroker {
    pub fn new(channel_name: impl Into<String>, timeout: Duration) -> Self {
        Self {
            channel_name: channel_name.into(),
            timeout,
        }
    }
}

#[async_trait]
impl ApprovalBroker for TextReplyApprovalBroker {
    async fn request_approval(
        &self,
        request: &ApprovalRequest,
    ) -> anyhow::Result<ApprovalDecision> {
        tracing::info!(
            channel = %self.channel_name,
            tool = %request.tool_name,
            risk = ?request.risk_level,
            timeout_secs = self.timeout.as_secs(),
            "tool approval requested via channel (auto-deny until interactive approval implemented)"
        );

        Ok(ApprovalDecision::Denied {
            reason: format!(
                "Channel '{}' approval not yet implemented. Set autonomy_level to 'full' or 'read_only' in config.",
                self.channel_name
            ),
        })
    }
}

#[must_use]
pub fn broker_for_channel(channel_name: &str) -> Arc<dyn ApprovalBroker> {
    match channel_name {
        "email" | "irc" | "webhook" => Arc::new(AutoDenyBroker {
            reason: format!(
                "Tool execution blocked: '{channel_name}' does not support interactive approval. Set autonomy_level to 'full' or 'read_only' in config."
            ),
        }),
        _ => Arc::new(TextReplyApprovalBroker::new(
            channel_name,
            Duration::from_secs(60),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{TextReplyApprovalBroker, broker_for_channel};
    use crate::security::approval::{ApprovalBroker, ApprovalDecision, ApprovalRequest, RiskLevel};
    use std::time::Duration;

    fn request_for_channel(channel: &str) -> ApprovalRequest {
        ApprovalRequest {
            intent_id: "intent-1".to_string(),
            tool_name: "shell".to_string(),
            args_summary: "ls".to_string(),
            risk_level: RiskLevel::High,
            entity_id: "entity-1".to_string(),
            channel: channel.to_string(),
        }
    }

    #[tokio::test]
    async fn broker_for_email_auto_denies_with_config_guidance() {
        let request = request_for_channel("email");
        let decision = broker_for_channel("email")
            .request_approval(&request)
            .await
            .expect("email broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("email broker should deny");
        };

        assert!(reason.contains("does not support interactive approval"));
        assert!(reason.contains("autonomy_level"));
    }

    #[tokio::test]
    async fn broker_for_irc_auto_denies_with_config_guidance() {
        let request = request_for_channel("irc");
        let decision = broker_for_channel("irc")
            .request_approval(&request)
            .await
            .expect("irc broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("irc broker should deny");
        };

        assert!(reason.contains("does not support interactive approval"));
        assert!(reason.contains("autonomy_level"));
    }

    #[tokio::test]
    async fn broker_for_webhook_auto_denies_with_config_guidance() {
        let request = request_for_channel("webhook");
        let decision = broker_for_channel("webhook")
            .request_approval(&request)
            .await
            .expect("webhook broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("webhook broker should deny");
        };

        assert!(reason.contains("does not support interactive approval"));
        assert!(reason.contains("autonomy_level"));
    }

    #[tokio::test]
    async fn broker_for_telegram_uses_text_reply_stub() {
        let request = request_for_channel("telegram");
        let decision = broker_for_channel("telegram")
            .request_approval(&request)
            .await
            .expect("telegram broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("telegram broker should currently deny");
        };

        assert!(reason.contains("approval not yet implemented"));
        assert!(reason.contains("telegram"));
    }

    #[tokio::test]
    async fn broker_for_discord_uses_text_reply_stub() {
        let request = request_for_channel("discord");
        let decision = broker_for_channel("discord")
            .request_approval(&request)
            .await
            .expect("discord broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("discord broker should currently deny");
        };

        assert!(reason.contains("approval not yet implemented"));
        assert!(reason.contains("discord"));
    }

    #[tokio::test]
    async fn text_reply_broker_denies_with_informative_message() {
        let broker = TextReplyApprovalBroker::new("slack", Duration::from_secs(60));
        let request = request_for_channel("slack");
        let decision = broker
            .request_approval(&request)
            .await
            .expect("text reply broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("text reply broker should currently deny");
        };

        assert!(reason.contains("approval not yet implemented"));
        assert!(reason.contains("autonomy_level"));
        assert_eq!(broker.timeout, Duration::from_secs(60));
    }
}
