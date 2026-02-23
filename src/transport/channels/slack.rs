use super::attachments::media_attachment_url;
use super::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};
use crate::transport::channels::policy::{AllowlistMatch, is_allowed_user};
use anyhow::Context;
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

/// Slack channel — polls conversations.history via Web API
pub struct SlackChannel {
    bot_token: String,
    channel_id: Option<String>,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl SlackChannel {
    pub fn new(bot_token: String, channel_id: Option<String>, allowed_users: Vec<String>) -> Self {
        Self {
            bot_token,
            channel_id,
            allowed_users,
            client: reqwest::Client::new(),
        }
    }

    /// Check if a Slack user ID is in the allowlist.
    /// Empty list means deny everyone until explicitly configured.
    /// `"*"` means allow everyone.
    fn is_user_allowed(&self, user_id: &str) -> bool {
        is_allowed_user(&self.allowed_users, user_id, AllowlistMatch::Exact)
    }

    /// Get the bot's own user ID so we can ignore our own messages
    async fn get_bot_user_id(&self) -> Option<String> {
        let resp: serde_json::Value = self
            .client
            .get("https://slack.com/api/auth.test")
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        resp.get("user_id")
            .and_then(|u| u.as_str())
            .map(String::from)
    }

    fn parse_files(msg: &Value) -> Vec<MediaAttachment> {
        msg.get("files")
            .and_then(Value::as_array)
            .map(|files| {
                files
                    .iter()
                    .filter_map(|file| {
                        let url = file.get("url_private").and_then(Value::as_str)?;
                        let mime_type = file.get("mimetype").and_then(Value::as_str);
                        let filename = file
                            .get("name")
                            .and_then(Value::as_str)
                            .map(ToString::to_string);
                        Some(media_attachment_url(url.to_string(), mime_type, filename))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str {
        "slack"
    }

    fn max_message_length(&self) -> usize {
        3000
    }

    async fn send(&self, message: &str, channel: &str) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "channel": channel,
            "text": message
        });

        let resp = self
            .client
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            anyhow::bail!("Slack chat.postMessage failed ({status}): {body}");
        }

        // Slack returns 200 for most app-level errors; check JSON "ok" field
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack chat.postMessage failed: {err}");
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let channel_id = self
            .channel_id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Slack channel_id required for listening"))?;

        let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
        let mut last_ts = String::new();

        tracing::info!("Slack channel listening on #{channel_id}...");

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            let mut params = vec![("channel", channel_id.clone()), ("limit", "10".to_string())];
            if !last_ts.is_empty() {
                params.push(("oldest", last_ts.clone()));
            }

            let resp = match self
                .client
                .get("https://slack.com/api/conversations.history")
                .bearer_auth(&self.bot_token)
                .query(&params)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Slack poll error: {e}");
                    continue;
                }
            };

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Slack parse error: {e}");
                    continue;
                }
            };

            if let Some(messages) = data.get("messages").and_then(|m| m.as_array()) {
                // Messages come newest-first, reverse to process oldest first
                for msg in messages.iter().rev() {
                    let ts = msg.get("ts").and_then(|t| t.as_str()).unwrap_or("");
                    let user = msg
                        .get("user")
                        .and_then(|u| u.as_str())
                        .unwrap_or("unknown");
                    let text = msg.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    let attachments = Self::parse_files(msg);

                    // Skip bot's own messages
                    if user == bot_user_id {
                        continue;
                    }

                    // Sender validation
                    if !self.is_user_allowed(user) {
                        tracing::warn!("Slack: ignoring message from unauthorized user: {user}");
                        continue;
                    }

                    // Skip empty or already-seen
                    if (text.is_empty() && attachments.is_empty()) || ts <= last_ts.as_str() {
                        continue;
                    }

                    last_ts = ts.to_string();

                    let channel_msg = ChannelMessage {
                        id: Uuid::new_v4().to_string(),
                        sender: channel_id.clone(),
                        content: text.to_string(),
                        channel: "slack".to_string(),
                        conversation_id: None,
                        thread_id: None,
                        reply_to: None,
                        message_id: None,
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        attachments,
                    };

                    if tx.send(channel_msg).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        self.client
            .get("https://slack.com/api/auth.test")
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn send_media(
        &self,
        attachment: &MediaAttachment,
        recipient: &str,
    ) -> anyhow::Result<()> {
        let bytes = match &attachment.data {
            MediaData::Url(media_url) => self
                .client
                .get(media_url)
                .bearer_auth(&self.bot_token)
                .send()
                .await
                .context("download Slack media before upload")?
                .bytes()
                .await
                .context("read Slack media bytes")?
                .to_vec(),
            MediaData::Bytes(raw_bytes) => raw_bytes.clone(),
        };

        let filename = attachment
            .filename
            .clone()
            .unwrap_or_else(|| "attachment".to_string());
        let file_part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
        let form = reqwest::multipart::Form::new()
            .text("channels", recipient.to_string())
            .part("file", file_part);

        let resp = self
            .client
            .post("https://slack.com/api/files.upload")
            .bearer_auth(&self.bot_token)
            .multipart(form)
            .send()
            .await
            .context("send Slack files.upload request")?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));

        if !status.is_success() {
            anyhow::bail!("Slack files.upload failed ({status}): {body}");
        }

        let parsed: Value = serde_json::from_str(&body).unwrap_or_default();
        if parsed.get("ok") == Some(&Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            anyhow::bail!("Slack files.upload failed: {err}");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_channel_name() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec![]);
        assert_eq!(ch.name(), "slack");
    }

    #[test]
    fn slack_channel_with_channel_id() {
        let ch = SlackChannel::new("xoxb-fake".into(), Some("C12345".into()), vec![]);
        assert_eq!(ch.channel_id, Some("C12345".to_string()));
    }

    #[test]
    fn empty_allowlist_denies_everyone() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec![]);
        assert!(!ch.is_user_allowed("U12345"));
        assert!(!ch.is_user_allowed("anyone"));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec!["*".into()]);
        assert!(ch.is_user_allowed("U12345"));
    }

    #[test]
    fn specific_allowlist_filters() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec!["U111".into(), "U222".into()]);
        assert!(ch.is_user_allowed("U111"));
        assert!(ch.is_user_allowed("U222"));
        assert!(!ch.is_user_allowed("U333"));
    }

    #[test]
    fn allowlist_exact_match_not_substring() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec!["U111".into()]);
        assert!(!ch.is_user_allowed("U1111"));
        assert!(!ch.is_user_allowed("U11"));
    }

    #[test]
    fn allowlist_empty_user_id() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec!["U111".into()]);
        assert!(!ch.is_user_allowed(""));
    }

    #[test]
    fn allowlist_case_sensitive() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec!["U111".into()]);
        assert!(ch.is_user_allowed("U111"));
        assert!(!ch.is_user_allowed("u111"));
    }

    #[test]
    fn allowlist_wildcard_and_specific() {
        let ch = SlackChannel::new("xoxb-fake".into(), None, vec!["U111".into(), "*".into()]);
        assert!(ch.is_user_allowed("U111"));
        assert!(ch.is_user_allowed("anyone"));
    }

    #[test]
    fn parse_files_extracts_media_attachment() {
        let msg = serde_json::json!({
            "files": [
                {
                    "id": "F123",
                    "name": "report.pdf",
                    "mimetype": "application/pdf",
                    "url_private": "https://files.slack.com/files-pri/T/F/report.pdf"
                }
            ]
        });

        let attachments = SlackChannel::parse_files(&msg);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].mime_type, "application/pdf");
        assert_eq!(attachments[0].filename.as_deref(), Some("report.pdf"));
        assert!(matches!(
            &attachments[0].data,
            MediaData::Url(url) if url.contains("files-pri")
        ));
    }

    #[test]
    fn parse_files_empty_array_returns_none() {
        let msg = serde_json::json!({ "files": [] });
        assert!(SlackChannel::parse_files(&msg).is_empty());
    }

    #[test]
    fn parse_files_missing_field_returns_none() {
        let msg = serde_json::json!({ "text": "hello" });
        assert!(SlackChannel::parse_files(&msg).is_empty());
    }

    #[test]
    fn parse_files_skips_entries_without_private_url() {
        let msg = serde_json::json!({
            "files": [
                {"id": "F1", "name": "no_url.txt", "mimetype": "text/plain"},
                {
                    "id": "F2",
                    "name": "with_url.txt",
                    "mimetype": "text/plain",
                    "url_private": "https://files.slack.com/files-pri/T/F/with_url.txt"
                }
            ]
        });

        let attachments = SlackChannel::parse_files(&msg);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].filename.as_deref(), Some("with_url.txt"));
    }

    #[test]
    fn parse_files_defaults_mime_type() {
        let msg = serde_json::json!({
            "files": [
                {
                    "id": "F123",
                    "name": "blob.bin",
                    "url_private": "https://files.slack.com/files-pri/T/F/blob.bin"
                }
            ]
        });

        let attachments = SlackChannel::parse_files(&msg);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].mime_type, "application/octet-stream");
    }

    #[tokio::test]
    async fn send_media_uses_files_upload_endpoint() {
        let ch = SlackChannel::new("xoxb-fake".into(), Some("C12345".into()), vec!["*".into()]);
        let attachment = MediaAttachment {
            mime_type: "text/plain".to_string(),
            data: MediaData::Bytes(b"hello".to_vec()),
            filename: Some("note.txt".to_string()),
        };

        let err = ch
            .send_media(&attachment, "C12345")
            .await
            .expect_err("network failure expected")
            .to_string();
        assert!(err.contains("files.upload") || err.contains("Slack"));
    }
}
