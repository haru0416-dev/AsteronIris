use crate::security::approval::{ApprovalBroker, ApprovalDecision, ApprovalRequest};
#[cfg(feature = "discord")]
use crate::security::approval_discord::DiscordApprovalBroker;
use crate::security::approval_telegram::TelegramApprovalBroker;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ChannelApprovalContext {
    pub bot_token: Option<String>,
    pub channel_id: Option<String>,
    pub timeout: Duration,
}

impl Default for ChannelApprovalContext {
    fn default() -> Self {
        Self {
            bot_token: None,
            channel_id: None,
            timeout: Duration::from_secs(60),
        }
    }
}

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
pub fn broker_for_channel(
    channel_name: &str,
    channel_config: &ChannelApprovalContext,
) -> Arc<dyn ApprovalBroker> {
    match channel_name {
        #[cfg(feature = "discord")]
        "discord" => channel_config
            .bot_token
            .as_deref()
            .zip(channel_config.channel_id.as_deref())
            .map_or_else(
                || {
                    Arc::new(TextReplyApprovalBroker::new(
                        channel_name,
                        channel_config.timeout,
                    )) as Arc<dyn ApprovalBroker>
                },
                |(bot_token, channel_id)| {
                    Arc::new(DiscordApprovalBroker::new(
                        bot_token,
                        channel_id,
                        channel_config.timeout,
                    )) as Arc<dyn ApprovalBroker>
                },
            ),
        "telegram" => channel_config
            .bot_token
            .as_deref()
            .zip(channel_config.channel_id.as_deref())
            .map_or_else(
                || {
                    Arc::new(TextReplyApprovalBroker::new(
                        channel_name,
                        channel_config.timeout,
                    )) as Arc<dyn ApprovalBroker>
                },
                |(bot_token, chat_id)| {
                    Arc::new(TelegramApprovalBroker::new(
                        bot_token,
                        chat_id,
                        channel_config.timeout,
                    )) as Arc<dyn ApprovalBroker>
                },
            ),
        _ => Arc::new(TextReplyApprovalBroker::new(
            channel_name,
            channel_config.timeout,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{ChannelApprovalContext, TextReplyApprovalBroker, broker_for_channel};
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

    #[test]
    fn channel_approval_context_default_values() {
        let context = ChannelApprovalContext::default();
        assert!(context.bot_token.is_none());
        assert!(context.channel_id.is_none());
        assert_eq!(context.timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn broker_for_email_uses_text_reply_auto_deny() {
        let request = request_for_channel("email");
        let decision = broker_for_channel("email", &ChannelApprovalContext::default())
            .request_approval(&request)
            .await
            .expect("email broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("email broker should deny");
        };

        assert!(reason.contains("approval not yet implemented"));
        assert!(reason.contains("autonomy_level"));
    }

    #[tokio::test]
    async fn broker_for_irc_uses_text_reply_auto_deny() {
        let request = request_for_channel("irc");
        let decision = broker_for_channel("irc", &ChannelApprovalContext::default())
            .request_approval(&request)
            .await
            .expect("irc broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("irc broker should deny");
        };

        assert!(reason.contains("approval not yet implemented"));
        assert!(reason.contains("autonomy_level"));
    }

    #[tokio::test]
    async fn broker_for_webhook_uses_text_reply_auto_deny() {
        let request = request_for_channel("webhook");
        let decision = broker_for_channel("webhook", &ChannelApprovalContext::default())
            .request_approval(&request)
            .await
            .expect("webhook broker should not fail");

        let ApprovalDecision::Denied { reason } = decision else {
            panic!("webhook broker should deny");
        };

        assert!(reason.contains("approval not yet implemented"));
        assert!(reason.contains("autonomy_level"));
    }

    #[tokio::test]
    async fn broker_for_telegram_without_context_falls_back_to_text_reply() {
        let request = request_for_channel("telegram");
        let decision = broker_for_channel("telegram", &ChannelApprovalContext::default())
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
    async fn broker_for_discord_without_context_falls_back_to_text_reply() {
        let request = request_for_channel("discord");
        let decision = broker_for_channel("discord", &ChannelApprovalContext::default())
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
    async fn broker_for_discord_with_context_uses_interactive_timeout_path() {
        let request = request_for_channel("discord");
        let context = ChannelApprovalContext {
            bot_token: Some("discord-token".to_string()),
            channel_id: Some("123".to_string()),
            timeout: Duration::ZERO,
        };
        let decision = broker_for_channel("discord", &context)
            .request_approval(&request)
            .await
            .expect("discord interactive broker should not fail on immediate timeout");

        assert_eq!(
            decision,
            ApprovalDecision::Denied {
                reason: "approval timed out".to_string()
            }
        );
    }

    #[tokio::test]
    async fn broker_for_telegram_with_context_uses_interactive_timeout_path() {
        let request = request_for_channel("telegram");
        let context = ChannelApprovalContext {
            bot_token: Some("telegram-token".to_string()),
            channel_id: Some("456".to_string()),
            timeout: Duration::ZERO,
        };
        let decision = broker_for_channel("telegram", &context)
            .request_approval(&request)
            .await
            .expect("telegram interactive broker should not fail on immediate timeout");

        assert_eq!(
            decision,
            ApprovalDecision::Denied {
                reason: "approval timed out".to_string()
            }
        );
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
