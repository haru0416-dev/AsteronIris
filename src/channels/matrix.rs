use crate::channels::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};
use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::mpsc;

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

#[derive(Debug, Deserialize)]
struct SyncResponse {
    next_batch: String,
    #[serde(default)]
    rooms: Rooms,
}

#[derive(Debug, Deserialize, Default)]
struct Rooms {
    #[serde(default)]
    join: std::collections::HashMap<String, JoinedRoom>,
}

#[derive(Debug, Deserialize)]
struct JoinedRoom {
    #[serde(default)]
    timeline: Timeline,
}

#[derive(Debug, Deserialize, Default)]
struct Timeline {
    #[serde(default)]
    events: Vec<TimelineEvent>,
}

#[derive(Debug, Deserialize)]
struct TimelineEvent {
    #[serde(rename = "type")]
    event_type: String,
    sender: String,
    #[serde(default)]
    content: EventContent,
}

#[derive(Debug, Deserialize, Default)]
struct EventContent {
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    msgtype: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    info: Option<EventContentInfo>,
}

#[derive(Debug, Deserialize, Default)]
struct EventContentInfo {
    #[serde(default)]
    mimetype: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WhoAmIResponse {
    user_id: String,
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
        if self.allowed_users.iter().any(|u| u == "*") {
            return true;
        }
        self.allowed_users
            .iter()
            .any(|u| u.eq_ignore_ascii_case(sender))
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

    fn mxc_to_http(&self, mxc_url: &str) -> Option<String> {
        let stripped = mxc_url.strip_prefix("mxc://")?;
        let (server, media_id) = stripped.split_once('/')?;
        Some(format!(
            "{}/_matrix/media/v3/download/{server}/{media_id}",
            self.homeserver
        ))
    }

    fn parse_media_attachments(&self, content: &EventContent) -> Vec<MediaAttachment> {
        let Some(msgtype) = content.msgtype.as_deref() else {
            return Vec::new();
        };

        if !matches!(msgtype, "m.image" | "m.audio" | "m.video" | "m.file") {
            return Vec::new();
        }

        let Some(mxc_url) = content.url.as_deref() else {
            return Vec::new();
        };
        let Some(download_url) = self.mxc_to_http(mxc_url) else {
            return Vec::new();
        };

        let mime_type = content
            .info
            .as_ref()
            .and_then(|info| info.mimetype.as_deref())
            .unwrap_or("application/octet-stream")
            .to_string();

        vec![MediaAttachment {
            mime_type,
            data: MediaData::Url(download_url),
            filename: content.body.clone(),
        }]
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

                    // Only process text messages
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel() -> MatrixChannel {
        MatrixChannel::new(
            "https://matrix.org".to_string(),
            "syt_test_token".to_string(),
            "!room:matrix.org".to_string(),
            vec!["@user:matrix.org".to_string()],
        )
    }

    #[test]
    fn creates_with_correct_fields() {
        let ch = make_channel();
        assert_eq!(ch.homeserver, "https://matrix.org");
        assert_eq!(ch.access_token, "syt_test_token");
        assert_eq!(ch.room_id, "!room:matrix.org");
        assert_eq!(ch.allowed_users.len(), 1);
    }

    #[test]
    fn strips_trailing_slash() {
        let ch = MatrixChannel::new(
            "https://matrix.org/".to_string(),
            "tok".to_string(),
            "!r:m".to_string(),
            vec![],
        );
        assert_eq!(ch.homeserver, "https://matrix.org");
    }

    #[test]
    fn no_trailing_slash_unchanged() {
        let ch = MatrixChannel::new(
            "https://matrix.org".to_string(),
            "tok".to_string(),
            "!r:m".to_string(),
            vec![],
        );
        assert_eq!(ch.homeserver, "https://matrix.org");
    }

    #[test]
    fn multiple_trailing_slashes_strips_one() {
        let ch = MatrixChannel::new(
            "https://matrix.org//".to_string(),
            "tok".to_string(),
            "!r:m".to_string(),
            vec![],
        );
        assert_eq!(ch.homeserver, "https://matrix.org/");
    }

    #[test]
    fn wildcard_allows_anyone() {
        let ch = MatrixChannel::new(
            "https://m.org".to_string(),
            "tok".to_string(),
            "!r:m".to_string(),
            vec!["*".to_string()],
        );
        assert!(ch.is_user_allowed("@anyone:matrix.org"));
        assert!(ch.is_user_allowed("@hacker:evil.org"));
    }

    #[test]
    fn specific_user_allowed() {
        let ch = make_channel();
        assert!(ch.is_user_allowed("@user:matrix.org"));
    }

    #[test]
    fn unknown_user_denied() {
        let ch = make_channel();
        assert!(!ch.is_user_allowed("@stranger:matrix.org"));
        assert!(!ch.is_user_allowed("@evil:hacker.org"));
    }

    #[test]
    fn user_case_insensitive() {
        let ch = MatrixChannel::new(
            "https://m.org".to_string(),
            "tok".to_string(),
            "!r:m".to_string(),
            vec!["@User:Matrix.org".to_string()],
        );
        assert!(ch.is_user_allowed("@user:matrix.org"));
        assert!(ch.is_user_allowed("@USER:MATRIX.ORG"));
    }

    #[test]
    fn empty_allowlist_denies_all() {
        let ch = MatrixChannel::new(
            "https://m.org".to_string(),
            "tok".to_string(),
            "!r:m".to_string(),
            vec![],
        );
        assert!(!ch.is_user_allowed("@anyone:matrix.org"));
    }

    #[test]
    fn name_returns_matrix() {
        let ch = make_channel();
        assert_eq!(ch.name(), "matrix");
    }

    #[test]
    fn sync_response_deserializes_empty() {
        let json = r#"{"next_batch":"s123","rooms":{"join":{}}}"#;
        let resp: SyncResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_batch, "s123");
        assert!(resp.rooms.join.is_empty());
    }

    #[test]
    fn sync_response_deserializes_with_events() {
        let json = r#"{
            "next_batch": "s456",
            "rooms": {
                "join": {
                    "!room:matrix.org": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.message",
                                    "sender": "@user:matrix.org",
                                    "content": {
                                        "msgtype": "m.text",
                                        "body": "Hello!"
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }"#;
        let resp: SyncResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.next_batch, "s456");
        let room = resp.rooms.join.get("!room:matrix.org").unwrap();
        assert_eq!(room.timeline.events.len(), 1);
        assert_eq!(room.timeline.events[0].sender, "@user:matrix.org");
        assert_eq!(
            room.timeline.events[0].content.body.as_deref(),
            Some("Hello!")
        );
        assert_eq!(
            room.timeline.events[0].content.msgtype.as_deref(),
            Some("m.text")
        );
    }

    #[test]
    fn sync_response_ignores_non_text_events() {
        let json = r#"{
            "next_batch": "s789",
            "rooms": {
                "join": {
                    "!room:m": {
                        "timeline": {
                            "events": [
                                {
                                    "type": "m.room.member",
                                    "sender": "@user:m",
                                    "content": {}
                                }
                            ]
                        }
                    }
                }
            }
        }"#;
        let resp: SyncResponse = serde_json::from_str(json).unwrap();
        let room = resp.rooms.join.get("!room:m").unwrap();
        assert_eq!(room.timeline.events[0].event_type, "m.room.member");
        assert!(room.timeline.events[0].content.body.is_none());
    }

    #[test]
    fn whoami_response_deserializes() {
        let json = r#"{"user_id":"@bot:matrix.org"}"#;
        let resp: WhoAmIResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.user_id, "@bot:matrix.org");
    }

    #[test]
    fn event_content_defaults() {
        let json = r#"{"type":"m.room.message","sender":"@u:m","content":{}}"#;
        let event: TimelineEvent = serde_json::from_str(json).unwrap();
        assert!(event.content.body.is_none());
        assert!(event.content.msgtype.is_none());
    }

    #[test]
    fn sync_response_missing_rooms_defaults() {
        let json = r#"{"next_batch":"s0"}"#;
        let resp: SyncResponse = serde_json::from_str(json).unwrap();
        assert!(resp.rooms.join.is_empty());
    }

    #[test]
    fn mxc_to_http_converts_valid_mxc_url() {
        let ch = make_channel();
        let http = ch.mxc_to_http("mxc://matrix.org/abc123");
        assert_eq!(
            http.as_deref(),
            Some("https://matrix.org/_matrix/media/v3/download/matrix.org/abc123")
        );
    }

    #[test]
    fn mxc_to_http_rejects_non_mxc_url() {
        let ch = make_channel();
        assert!(ch.mxc_to_http("https://matrix.org/media").is_none());
    }

    #[test]
    fn parse_media_attachments_for_image_event() {
        let ch = make_channel();
        let content = EventContent {
            body: Some("photo.png".to_string()),
            msgtype: Some("m.image".to_string()),
            url: Some("mxc://matrix.org/image123".to_string()),
            info: Some(EventContentInfo {
                mimetype: Some("image/png".to_string()),
            }),
        };

        let attachments = ch.parse_media_attachments(&content);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].mime_type, "image/png");
        assert_eq!(attachments[0].filename.as_deref(), Some("photo.png"));
        assert!(matches!(
            &attachments[0].data,
            MediaData::Url(url) if url.contains("/download/matrix.org/image123")
        ));
    }

    #[test]
    fn parse_media_attachments_for_file_event() {
        let ch = make_channel();
        let content = EventContent {
            body: Some("doc.pdf".to_string()),
            msgtype: Some("m.file".to_string()),
            url: Some("mxc://matrix.org/file123".to_string()),
            info: Some(EventContentInfo {
                mimetype: Some("application/pdf".to_string()),
            }),
        };

        let attachments = ch.parse_media_attachments(&content);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].mime_type, "application/pdf");
        assert_eq!(attachments[0].filename.as_deref(), Some("doc.pdf"));
    }

    #[test]
    fn parse_media_attachments_text_event_has_no_attachments() {
        let ch = make_channel();
        let content = EventContent {
            body: Some("hello".to_string()),
            msgtype: Some("m.text".to_string()),
            url: None,
            info: None,
        };

        let attachments = ch.parse_media_attachments(&content);
        assert!(attachments.is_empty());
        assert_eq!(content.body.as_deref(), Some("hello"));
    }

    #[test]
    fn parse_media_attachments_requires_url_for_media_msgtypes() {
        let ch = make_channel();
        let content = EventContent {
            body: Some("clip.mp4".to_string()),
            msgtype: Some("m.video".to_string()),
            url: None,
            info: Some(EventContentInfo {
                mimetype: Some("video/mp4".to_string()),
            }),
        };

        assert!(ch.parse_media_attachments(&content).is_empty());
    }

    #[test]
    fn parse_media_attachments_defaults_mime_type() {
        let ch = make_channel();
        let content = EventContent {
            body: Some("audio.ogg".to_string()),
            msgtype: Some("m.audio".to_string()),
            url: Some("mxc://matrix.org/audio999".to_string()),
            info: None,
        };

        let attachments = ch.parse_media_attachments(&content);
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].mime_type, "application/octet-stream");
    }

    #[test]
    fn event_content_deserializes_media_fields() {
        let json = r#"{
            "type":"m.room.message",
            "sender":"@u:m",
            "content":{
                "msgtype":"m.image",
                "body":"cat.png",
                "url":"mxc://matrix.org/cat123",
                "info":{"mimetype":"image/png"}
            }
        }"#;

        let event: TimelineEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.content.msgtype.as_deref(), Some("m.image"));
        assert_eq!(event.content.body.as_deref(), Some("cat.png"));
        assert_eq!(
            event.content.url.as_deref(),
            Some("mxc://matrix.org/cat123")
        );
        assert_eq!(
            event
                .content
                .info
                .as_ref()
                .and_then(|info| info.mimetype.as_deref()),
            Some("image/png")
        );
    }
}
