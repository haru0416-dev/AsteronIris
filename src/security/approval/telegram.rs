use crate::security::approval::{ApprovalBroker, ApprovalDecision, ApprovalRequest};
use anyhow::{Context, Result};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

pub struct TelegramApprovalBroker {
    pub bot_token: String,
    pub chat_id: String,
    pub client: reqwest::Client,
    pub timeout: Duration,
}

impl TelegramApprovalBroker {
    #[must_use]
    pub fn new(
        bot_token: impl Into<String>,
        chat_id: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            bot_token: bot_token.into(),
            chat_id: chat_id.into(),
            client: reqwest::Client::new(),
            timeout,
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.bot_token)
    }

    fn approval_message_payload(&self, request: &ApprovalRequest) -> Value {
        serde_json::json!({
            "chat_id": self.chat_id,
            "text": format!(
                "Tool approval required\nTool: {}\nArgs: {}\nRisk: {:?}\nEntity: {}",
                request.tool_name, request.args_summary, request.risk_level, request.entity_id
            ),
            "reply_markup": {
                "inline_keyboard": [[
                    {"text": "✅ Approve", "callback_data": "approve"},
                    {"text": "❌ Deny", "callback_data": "deny"}
                ]]
            }
        })
    }

    pub async fn send_approval_message(&self, request: &ApprovalRequest) -> Result<i64> {
        let response = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&self.approval_message_payload(request))
            .send()
            .await
            .context("send Telegram approval message")?
            .error_for_status()
            .context("Telegram sendMessage rejected")?;

        let body: Value = response
            .json()
            .await
            .context("parse Telegram sendMessage response")?;
        let message_id = body
            .get("result")
            .and_then(|result| result.get("message_id"))
            .and_then(Value::as_i64)
            .context("Telegram approval response missing message_id")?;
        Ok(message_id)
    }

    fn extract_callback_for_message(
        update: &Value,
        target_message_id: i64,
    ) -> Option<(String, String)> {
        let callback = update.get("callback_query")?;
        let message_id = callback
            .get("message")
            .and_then(|message| message.get("message_id"))
            .and_then(Value::as_i64)?;
        if message_id != target_message_id {
            return None;
        }

        let callback_id = callback.get("id").and_then(Value::as_str)?;
        let data = callback.get("data").and_then(Value::as_str)?;
        Some((callback_id.to_string(), data.to_string()))
    }

    async fn acknowledge_callback(&self, callback_id: &str) -> Result<()> {
        self.client
            .post(self.api_url("answerCallbackQuery"))
            .json(&serde_json::json!({ "callback_query_id": callback_id }))
            .send()
            .await
            .context("acknowledge Telegram callback query")?
            .error_for_status()
            .context("Telegram answerCallbackQuery rejected")?;
        Ok(())
    }

    pub async fn poll_callback_query(&self, target_message_id: i64) -> Result<Option<String>> {
        let deadline = Instant::now() + self.timeout;
        let mut offset: i64 = 0;

        while Instant::now() < deadline {
            let response = self
                .client
                .post(self.api_url("getUpdates"))
                .json(&serde_json::json!({
                    "offset": offset,
                    "timeout": 1,
                    "allowed_updates": ["callback_query"]
                }))
                .send()
                .await
                .context("poll Telegram callback updates")?
                .error_for_status()
                .context("Telegram getUpdates rejected")?;

            let body: Value = response
                .json()
                .await
                .context("parse Telegram getUpdates response")?;

            if let Some(updates) = body.get("result").and_then(Value::as_array) {
                for update in updates {
                    if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                        offset = update_id + 1;
                    }

                    if let Some((callback_id, data)) =
                        Self::extract_callback_for_message(update, target_message_id)
                    {
                        self.acknowledge_callback(&callback_id).await?;
                        return Ok(Some(data));
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(None)
    }
}

impl ApprovalBroker for TelegramApprovalBroker {
    fn request_approval<'a>(
        &'a self,
        request: &'a ApprovalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ApprovalDecision>> + Send + 'a>> {
        Box::pin(async move {
            if self.timeout.is_zero() {
                return Ok(ApprovalDecision::Denied {
                    reason: "approval timed out".to_string(),
                });
            }

            let message_id = self.send_approval_message(request).await?;
            match self.poll_callback_query(message_id).await? {
                Some(decision) if decision == "approve" => Ok(ApprovalDecision::Approved),
                Some(decision) if decision == "deny" => Ok(ApprovalDecision::Denied {
                    reason: "denied by user".to_string(),
                }),
                Some(decision) => Ok(ApprovalDecision::Denied {
                    reason: format!("unrecognized approval action: {decision}"),
                }),
                None => Ok(ApprovalDecision::Denied {
                    reason: "approval timed out".to_string(),
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramApprovalBroker;
    use crate::security::approval::{ApprovalBroker, ApprovalDecision, ApprovalRequest, RiskLevel};
    use std::time::Duration;

    fn test_request() -> ApprovalRequest {
        ApprovalRequest {
            intent_id: "intent-1".to_string(),
            tool_name: "file_write".to_string(),
            args_summary: "write 10 bytes to out.txt".to_string(),
            risk_level: RiskLevel::Medium,
            entity_id: "telegram:123".to_string(),
            channel: "telegram".to_string(),
        }
    }

    #[test]
    fn telegram_broker_constructs() {
        let broker = TelegramApprovalBroker::new("token", "chat", Duration::from_secs(9));
        assert_eq!(broker.bot_token, "token");
        assert_eq!(broker.chat_id, "chat");
        assert_eq!(broker.timeout, Duration::from_secs(9));
    }

    #[test]
    fn telegram_payload_contains_inline_keyboard() {
        let broker = TelegramApprovalBroker::new("token", "chat-1", Duration::from_secs(30));
        let payload = broker.approval_message_payload(&test_request());

        assert_eq!(payload["chat_id"], "chat-1");
        assert_eq!(
            payload["reply_markup"]["inline_keyboard"][0][0]["text"],
            "✅ Approve"
        );
        assert_eq!(
            payload["reply_markup"]["inline_keyboard"][0][0]["callback_data"],
            "approve"
        );
        assert_eq!(
            payload["reply_markup"]["inline_keyboard"][0][1]["text"],
            "❌ Deny"
        );
        assert_eq!(
            payload["reply_markup"]["inline_keyboard"][0][1]["callback_data"],
            "deny"
        );
    }

    #[test]
    fn telegram_payload_contains_request_fields() {
        let broker = TelegramApprovalBroker::new("token", "chat-2", Duration::from_secs(30));
        let payload = broker.approval_message_payload(&test_request());
        let text = payload["text"].as_str().unwrap_or_default();

        assert!(text.contains("Tool: file_write"));
        assert!(text.contains("Args: write 10 bytes to out.txt"));
        assert!(text.contains("Risk: Medium"));
        assert!(text.contains("Entity: telegram:123"));
    }

    #[test]
    fn telegram_extract_callback_accepts_matching_message() {
        let update = serde_json::json!({
            "update_id": 10,
            "callback_query": {
                "id": "cb-1",
                "data": "approve",
                "message": {"message_id": 42}
            }
        });

        let extracted = TelegramApprovalBroker::extract_callback_for_message(&update, 42);
        assert_eq!(extracted, Some(("cb-1".to_string(), "approve".to_string())));
    }

    #[test]
    fn telegram_extract_callback_ignores_other_messages() {
        let update = serde_json::json!({
            "callback_query": {
                "id": "cb-2",
                "data": "deny",
                "message": {"message_id": 77}
            }
        });

        let extracted = TelegramApprovalBroker::extract_callback_for_message(&update, 42);
        assert!(extracted.is_none());
    }

    #[test]
    fn telegram_extract_callback_requires_data_field() {
        let update = serde_json::json!({
            "callback_query": {
                "id": "cb-3",
                "message": {"message_id": 42}
            }
        });

        let extracted = TelegramApprovalBroker::extract_callback_for_message(&update, 42);
        assert!(extracted.is_none());
    }

    #[tokio::test]
    async fn telegram_timeout_path_denies_without_http() {
        let broker = TelegramApprovalBroker::new("token", "chat", Duration::ZERO);
        let decision = broker.request_approval(&test_request()).await.unwrap();
        assert_eq!(
            decision,
            ApprovalDecision::Denied {
                reason: "approval timed out".to_string()
            }
        );
    }
}
