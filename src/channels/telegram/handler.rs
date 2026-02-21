use super::TelegramChannel;
use crate::channels::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

impl TelegramChannel {
    pub(crate) fn telegram_file_url(&self, file_path: &str) -> String {
        format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.bot_token, file_path
        )
    }

    async fn get_file_download_url(&self, file_id: &str) -> Option<String> {
        #[cfg(test)]
        if let Some(file_path) = file_id.strip_prefix("test_file_path:") {
            return Some(self.telegram_file_url(file_path));
        }

        let resp = self
            .client
            .post(self.api_url("getFile"))
            .json(&serde_json::json!({ "file_id": file_id }))
            .send()
            .await
            .ok()?;

        if !resp.status().is_success() {
            return None;
        }

        let data: Value = resp.json().await.ok()?;
        let file_path = data
            .get("result")
            .and_then(|result| result.get("file_path"))
            .and_then(Value::as_str)?;

        Some(self.telegram_file_url(file_path))
    }

    pub async fn parse_telegram_attachments(&self, message: &Value) -> Vec<MediaAttachment> {
        let mut attachments = Vec::new();

        if let Some(photo_sizes) = message.get("photo").and_then(Value::as_array)
            && let Some(largest) = photo_sizes.last()
            && let Some(file_id) = largest.get("file_id").and_then(Value::as_str)
            && let Some(url) = self.get_file_download_url(file_id).await
        {
            attachments.push(MediaAttachment {
                mime_type: "image/jpeg".to_string(),
                data: MediaData::Url(url),
                filename: None,
            });
        }

        if let Some(document) = message.get("document")
            && let Some(file_id) = document.get("file_id").and_then(Value::as_str)
            && let Some(url) = self.get_file_download_url(file_id).await
        {
            let mime_type = document
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream")
                .to_string();
            let filename = document
                .get("file_name")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            attachments.push(MediaAttachment {
                mime_type,
                data: MediaData::Url(url),
                filename,
            });
        }

        if let Some(audio) = message.get("audio")
            && let Some(file_id) = audio.get("file_id").and_then(Value::as_str)
            && let Some(url) = self.get_file_download_url(file_id).await
        {
            let mime_type = audio
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("audio/mpeg")
                .to_string();
            let filename = audio
                .get("file_name")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            attachments.push(MediaAttachment {
                mime_type,
                data: MediaData::Url(url),
                filename,
            });
        }

        if let Some(voice) = message.get("voice")
            && let Some(file_id) = voice.get("file_id").and_then(Value::as_str)
            && let Some(url) = self.get_file_download_url(file_id).await
        {
            let mime_type = voice
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("audio/ogg")
                .to_string();
            attachments.push(MediaAttachment {
                mime_type,
                data: MediaData::Url(url),
                filename: Some("voice.ogg".to_string()),
            });
        }

        if let Some(video) = message.get("video")
            && let Some(file_id) = video.get("file_id").and_then(Value::as_str)
            && let Some(url) = self.get_file_download_url(file_id).await
        {
            let mime_type = video
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("video/mp4")
                .to_string();
            let filename = video
                .get("file_name")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            attachments.push(MediaAttachment {
                mime_type,
                data: MediaData::Url(url),
                filename,
            });
        }

        attachments
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn max_message_length(&self) -> usize {
        4096
    }

    async fn send(&self, message: &str, chat_id: &str) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": message,
            "parse_mode": "Markdown"
        });

        let resp = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            anyhow::bail!("Telegram sendMessage failed ({status}): {err}");
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut offset: i64 = 0;

        tracing::info!("Telegram channel listening for messages...");

        loop {
            let url = self.api_url("getUpdates");
            let body = serde_json::json!({
                "offset": offset,
                "timeout": 30,
                "allowed_updates": ["message"]
            });

            let resp = match self.client.post(&url).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Telegram poll error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Telegram parse error: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if let Some(results) = data.get("result").and_then(serde_json::Value::as_array) {
                for update in results {
                    // Advance offset past this update
                    if let Some(uid) = update.get("update_id").and_then(serde_json::Value::as_i64) {
                        offset = uid + 1;
                    }

                    let Some(message) = update.get("message") else {
                        continue;
                    };

                    let username_opt = message
                        .get("from")
                        .and_then(|f| f.get("username"))
                        .and_then(|u| u.as_str());
                    let username = username_opt.unwrap_or("unknown");

                    let user_id = message
                        .get("from")
                        .and_then(|f| f.get("id"))
                        .and_then(serde_json::Value::as_i64);
                    let user_id_str = user_id.map(|id| id.to_string());

                    let mut identities = vec![username];
                    if let Some(ref id) = user_id_str {
                        identities.push(id.as_str());
                    }

                    if !self.is_any_user_allowed(identities.iter().copied()) {
                        tracing::warn!(
                            "Telegram: ignoring message from unauthorized user: username={username}, user_id={}. \
 Allowlist Telegram @username or numeric user ID, then run `asteroniris onboard --channels-only`.",
                            user_id_str.as_deref().unwrap_or("unknown")
                        );
                        continue;
                    }

                    let text = message
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| message.get("caption").and_then(serde_json::Value::as_str))
                        .unwrap_or("");

                    let attachments = self.parse_telegram_attachments(message).await;
                    if text.is_empty() && attachments.is_empty() {
                        continue;
                    }

                    let chat_id = message
                        .get("chat")
                        .and_then(|c| c.get("id"))
                        .and_then(serde_json::Value::as_i64)
                        .map(|id| id.to_string())
                        .unwrap_or_default();

                    let msg = ChannelMessage {
                        id: Uuid::new_v4().to_string(),
                        sender: chat_id,
                        content: text.to_string(),
                        channel: "telegram".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        attachments,
                    };

                    if tx.send(msg).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        self.client
            .get(self.api_url("getMe"))
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
        let mime = attachment.mime_type.as_str();
        let filename = attachment.filename.as_deref().unwrap_or("attachment");

        match &attachment.data {
            MediaData::Url(url) => {
                if mime.starts_with("image/") {
                    self.send_photo_by_url(recipient, url, None).await
                } else if mime.starts_with("audio/") {
                    self.send_audio_by_url(recipient, url, None).await
                } else if mime.starts_with("video/") {
                    self.send_video_by_url(recipient, url, None).await
                } else {
                    self.send_document_by_url(recipient, url, None).await
                }
            }
            MediaData::Bytes(bytes) => {
                if mime.starts_with("image/") {
                    self.send_photo_bytes(recipient, bytes.clone(), filename, None)
                        .await
                } else if mime.starts_with("audio/") {
                    self.send_audio_bytes(recipient, bytes.clone(), filename, None)
                        .await
                } else if mime.starts_with("video/") {
                    self.send_video_bytes(recipient, bytes.clone(), filename, None)
                        .await
                } else {
                    self.send_document_bytes(recipient, bytes.clone(), filename, None)
                        .await
                }
            }
        }
    }
}
