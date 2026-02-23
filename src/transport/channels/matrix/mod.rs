mod media;
mod models;

#[cfg(test)]
mod tests;

use crate::transport::channels::policy::{AllowlistMatch, is_allowed_user};
use crate::transport::channels::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};
use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;

use self::models::{EventContent, SyncResponse, WhoAmIResponse};

/// Matrix channel using the Client-Server API (no SDK needed).
/// Connects to any Matrix homeserver (Element, Synapse, etc.).
#[derive(Clone)]
pub struct MatrixChannel {
    homeserver: String,
    access_token: String,
    room_id: String,
    allowed_users: Vec<String>,
    client: Client,
}

impl MatrixChannel {
    pub fn new(
        homeserver: String,
        access_token: String,
        room_id: String,
        allowed_users: Vec<String>,
    ) -> Self {
        let homeserver = if homeserver.ends_with('/') {
            homeserver[..homeserver.len() - 1].to_string()
        } else {
            homeserver
        };
        Self {
            homeserver,
            access_token,
            room_id,
            allowed_users,
            client: Client::new(),
        }
    }

    fn is_user_allowed(&self, sender: &str) -> bool {
        is_allowed_user(
            &self.allowed_users,
            sender,
            AllowlistMatch::AsciiCaseInsensitive,
        )
    }

    async fn get_my_user_id(&self) -> anyhow::Result<String> {
        let url = format!("{}/_matrix/client/v3/account/whoami", self.homeserver);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .context("send Matrix whoami request")?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Matrix whoami failed: {err}");
        }

        let who: WhoAmIResponse = resp.json().await.context("parse Matrix whoami response")?;
        Ok(who.user_id)
    }

    #[cfg(test)]
    fn mxc_to_http(&self, mxc_url: &str) -> Option<String> {
        media::mxc_to_http(&self.homeserver, mxc_url)
    }

    fn parse_media_attachments(&self, content: &EventContent) -> Vec<MediaAttachment> {
        media::parse_media_attachments(&self.homeserver, content)
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn name(&self) -> &str {
        "matrix"
    }

    fn max_message_length(&self) -> usize {
        60_000
    }

    async fn send(&self, message: &str, _target: &str) -> anyhow::Result<()> {
        let txn_id = format!("zc_{}", chrono::Utc::now().timestamp_millis());
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.homeserver, self.room_id, txn_id
        );

        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": message
        });

        let resp = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&body)
            .send()
            .await
            .context("send Matrix room message")?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Matrix send failed: {err}");
        }

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        tracing::info!("Matrix channel listening on room {}...", self.room_id);

        let my_user_id = self
            .get_my_user_id()
            .await
            .context("get Matrix user identity")?;

        // Initial sync to get the since token
        let url = format!(
            "{}/_matrix/client/v3/sync?timeout=30000&filter={{\"room\":{{\"timeline\":{{\"limit\":1}}}}}}",
            self.homeserver
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
            .context("send Matrix initial sync request")?;

        if !resp.status().is_success() {
            let err = resp.text().await?;
            anyhow::bail!("Matrix initial sync failed: {err}");
        }

        let sync: SyncResponse = resp
            .json()
            .await
            .context("parse Matrix initial sync response")?;
        let mut since = sync.next_batch;

        // Long-poll loop
        loop {
            let url = format!(
                "{}/_matrix/client/v3/sync?since={}&timeout=30000",
                self.homeserver, since
            );

            let resp = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Matrix sync error: {e}, retrying...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            if !resp.status().is_success() {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }

            let sync: SyncResponse = resp.json().await.context("parse Matrix sync response")?;
            since = sync.next_batch;

            // Process events from our room
            if let Some(room) = sync.rooms.join.get(&self.room_id) {
                for event in &room.timeline.events {
                    // Skip our own messages
                    if event.sender == my_user_id {
                        continue;
                    }

                    // Only process text and media messages
                    if event.event_type != "m.room.message" {
                        continue;
                    }

                    let msgtype = event.content.msgtype.as_deref().unwrap_or("");
                    if !matches!(
                        msgtype,
                        "m.text" | "m.image" | "m.audio" | "m.video" | "m.file"
                    ) {
                        continue;
                    }

                    if !self.is_user_allowed(&event.sender) {
                        continue;
                    }

                    let attachments = self.parse_media_attachments(&event.content);
                    let body = event.content.body.clone().unwrap_or_default();
                    if body.is_empty() && attachments.is_empty() {
                        continue;
                    }

                    let msg = ChannelMessage {
                        id: format!("mx_{}", chrono::Utc::now().timestamp_millis()),
                        sender: event.sender.clone(),
                        content: body,
                        channel: "matrix".to_string(),
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

                    if tx.send(msg).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/_matrix/client/v3/account/whoami", self.homeserver);
        let Ok(resp) = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .send()
            .await
        else {
            return false;
        };

        resp.status().is_success()
    }

    async fn send_media(
        &self,
        attachment: &MediaAttachment,
        _recipient: &str,
    ) -> anyhow::Result<()> {
        let bytes = match &attachment.data {
            MediaData::Url(media_url) => self
                .client
                .get(media_url)
                .send()
                .await
                .context("download Matrix media before upload")?
                .bytes()
                .await
                .context("read Matrix media bytes")?
                .to_vec(),
            MediaData::Bytes(raw_bytes) => raw_bytes.clone(),
        };

        let upload_url = format!("{}/_matrix/media/v3/upload", self.homeserver);
        let upload_resp = self
            .client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", attachment.mime_type.clone())
            .body(bytes.clone())
            .send()
            .await
            .context("send Matrix upload request")?;

        if !upload_resp.status().is_success() {
            let err = upload_resp.text().await?;
            anyhow::bail!("Matrix media upload failed: {err}");
        }

        let upload_data: serde_json::Value = upload_resp
            .json()
            .await
            .context("parse Matrix upload response")?;
        let Some(content_uri) = upload_data
            .get("content_uri")
            .and_then(serde_json::Value::as_str)
        else {
            anyhow::bail!("Matrix upload response missing content_uri");
        };

        let txn_id = format!("zc_{}", chrono::Utc::now().timestamp_millis());
        let send_url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.homeserver, self.room_id, txn_id
        );
        let filename = attachment
            .filename
            .clone()
            .unwrap_or_else(|| "attachment".to_string());
        let msgtype = if attachment.mime_type.starts_with("image/") {
            "m.image"
        } else if attachment.mime_type.starts_with("audio/") {
            "m.audio"
        } else if attachment.mime_type.starts_with("video/") {
            "m.video"
        } else {
            "m.file"
        };

        let body = serde_json::json!({
            "msgtype": msgtype,
            "body": filename,
            "url": content_uri,
            "info": {
                "mimetype": attachment.mime_type,
                "size": bytes.len()
            }
        });

        let send_resp = self
            .client
            .put(&send_url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .json(&body)
            .send()
            .await
            .context("send Matrix media message")?;

        if !send_resp.status().is_success() {
            let err = send_resp.text().await?;
            anyhow::bail!("Matrix send media failed: {err}");
        }

        Ok(())
    }
}
