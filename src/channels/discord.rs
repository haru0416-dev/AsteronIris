use super::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};
use anyhow::Context;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// Discord channel — connects via Gateway WebSocket for real-time messages
pub struct DiscordChannel {
    bot_token: String,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl DiscordChannel {
    pub fn new(bot_token: String, guild_id: Option<String>, allowed_users: Vec<String>) -> Self {
        Self {
            bot_token,
            guild_id,
            allowed_users,
            client: reqwest::Client::new(),
        }
    }

    /// Check if a Discord user ID is in the allowlist.
    /// Empty list means deny everyone until explicitly configured.
    /// `"*"` means allow everyone.
    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    fn bot_user_id_from_token(token: &str) -> Option<String> {
        let part = token.split('.').next()?;
        base64_decode(part)
    }

    pub async fn send_embed(
        &self,
        channel_id: &str,
        title: Option<&str>,
        description: &str,
        color: Option<u32>,
    ) -> anyhow::Result<()> {
        let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");
        let mut embed = json!({ "description": description });
        if let Some(t) = title {
            embed["title"] = json!(t);
        }
        if let Some(c) = color {
            embed["color"] = json!(c);
        }
        let body = json!({ "embeds": [embed] });
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(&body)
            .send()
            .await
            .context("send Discord embed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            anyhow::bail!("Discord embed send failed ({status}): {err}");
        }

        Ok(())
    }

    fn parse_attachments(d: &serde_json::Value) -> Vec<MediaAttachment> {
        d.get("attachments")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|att| {
                        let url = att.get("url")?.as_str()?.to_string();
                        let mime = att
                            .get("content_type")
                            .and_then(|c| c.as_str())
                            .unwrap_or("application/octet-stream")
                            .to_string();
                        let filename = att
                            .get("filename")
                            .and_then(|f| f.as_str())
                            .map(String::from);
                        Some(MediaAttachment {
                            mime_type: mime,
                            data: MediaData::Url(url),
                            filename,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Minimal base64 decode (no extra dep) — only needs to decode the user ID portion
#[allow(clippy::cast_possible_truncation)]
fn base64_decode(input: &str) -> Option<String> {
    let padded = match input.len() % 4 {
        2 => format!("{input}=="),
        3 => format!("{input}="),
        _ => input.to_string(),
    };

    let mut bytes = Vec::new();
    let chars: Vec<u8> = padded.bytes().collect();

    for chunk in chars.chunks(4) {
        if chunk.len() < 4 {
            break;
        }

        let mut v = [0usize; 4];
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                v[i] = 0;
            } else {
                v[i] = BASE64_ALPHABET.iter().position(|&a| a == b)?;
            }
        }

        bytes.push(((v[0] << 2) | (v[1] >> 4)) as u8);
        if chunk[2] != b'=' {
            bytes.push((((v[1] & 0xF) << 4) | (v[2] >> 2)) as u8);
        }
        if chunk[3] != b'=' {
            bytes.push((((v[2] & 0x3) << 6) | v[3]) as u8);
        }
    }

    String::from_utf8(bytes).ok()
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    fn max_message_length(&self) -> usize {
        2000
    }

    async fn send(&self, message: &str, channel_id: &str) -> anyhow::Result<()> {
        let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");
        let body = json!({ "content": message });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(&body)
            .send()
            .await
            .context("send Discord message request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            anyhow::bail!("Discord send message failed ({status}): {err}");
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let bot_user_id = Self::bot_user_id_from_token(&self.bot_token).unwrap_or_default();

        // Get Gateway URL
        let gw_resp: serde_json::Value = self
            .client
            .get("https://discord.com/api/v10/gateway/bot")
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await
            .context("fetch Discord gateway URL")?
            .json()
            .await
            .context("parse Discord gateway response")?;

        let gw_url = gw_resp
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("wss://gateway.discord.gg");

        let ws_url = format!("{gw_url}/?v=10&encoding=json");
        tracing::info!("Discord: connecting to gateway...");

        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .context("connect to Discord gateway WebSocket")?;
        let (mut write, mut read) = ws_stream.split();

        // Read Hello (opcode 10)
        let hello = read
            .next()
            .await
            .ok_or(anyhow::anyhow!("No hello"))
            .context("read Discord gateway hello message")??;
        let hello_data: serde_json::Value = serde_json::from_str(&hello.to_string())
            .context("parse Discord gateway hello event")?;
        let heartbeat_interval = hello_data
            .get("d")
            .and_then(|d| d.get("heartbeat_interval"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(41250);

        // Send Identify (opcode 2)
        let identify = json!({
            "op": 2,
            "d": {
                "token": self.bot_token,
                "intents": 37377, // GUILDS | GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT
                "properties": {
                    "os": "linux",
                    "browser": "asteroniris",
                    "device": "asteroniris"
                }
            }
        });
        write
            .send(Message::Text(identify.to_string().into()))
            .await
            .context("send Discord gateway identify")?;

        tracing::info!("Discord: connected and identified");

        // Track the last sequence number for heartbeats and resume.
        // Only accessed in the select! loop below, so a plain i64 suffices.
        let mut sequence: i64 = -1;

        // Spawn heartbeat timer — sends a tick signal, actual heartbeat
        // is assembled in the select! loop where `sequence` lives.
        let (hb_tx, mut hb_rx) = tokio::sync::mpsc::channel::<()>(1);
        let hb_interval = heartbeat_interval;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(hb_interval));
            loop {
                interval.tick().await;
                if hb_tx.send(()).await.is_err() {
                    break;
                }
            }
        });

        let guild_filter = self.guild_id.clone();

        loop {
            tokio::select! {
                _ = hb_rx.recv() => {
                    let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                    let hb = json!({"op": 1, "d": d});
                    if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                        break;
                    }
                }
                msg = read.next() => {
                    let msg = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => continue,
                    };

                    let event: serde_json::Value = match serde_json::from_str(&msg) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    // Track sequence number from all dispatch events
                    if let Some(s) = event.get("s").and_then(serde_json::Value::as_i64) {
                        sequence = s;
                    }

                    let op = event.get("op").and_then(serde_json::Value::as_u64).unwrap_or(0);

                    match op {
                        // Op 1: Server requests an immediate heartbeat
                        1 => {
                            let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                            let hb = json!({"op": 1, "d": d});
                            if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        // Op 7: Reconnect
                        7 => {
                            tracing::warn!("Discord: received Reconnect (op 7), closing for restart");
                            break;
                        }
                        // Op 9: Invalid Session
                        9 => {
                            tracing::warn!("Discord: received Invalid Session (op 9), closing for restart");
                            break;
                        }
                        _ => {}
                    }

                    // Only handle MESSAGE_CREATE (opcode 0, type "MESSAGE_CREATE")
                    let event_type = event.get("t").and_then(|t| t.as_str()).unwrap_or("");
                    if event_type != "MESSAGE_CREATE" {
                        continue;
                    }

                    let Some(d) = event.get("d") else {
                        continue;
                    };

                    // Skip messages from the bot itself
                    let author_id = d.get("author").and_then(|a| a.get("id")).and_then(|i| i.as_str()).unwrap_or("");
                    if author_id == bot_user_id {
                        continue;
                    }

                    // Skip bot messages
                    if d.get("author").and_then(|a| a.get("bot")).and_then(serde_json::Value::as_bool).unwrap_or(false) {
                        continue;
                    }

                    // Sender validation
                    if !self.is_user_allowed(author_id) {
                        tracing::warn!("Discord: ignoring message from unauthorized user: {author_id}");
                        continue;
                    }

                    // Guild filter
                    if let Some(ref gid) = guild_filter {
                        let msg_guild = d.get("guild_id").and_then(serde_json::Value::as_str).unwrap_or("");
                        if msg_guild != gid {
                            continue;
                        }
                    }

                    let content = d.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let attachments = Self::parse_attachments(d);

                    if content.is_empty() && attachments.is_empty() {
                        continue;
                    }

                    let channel_id = d.get("channel_id").and_then(|c| c.as_str()).unwrap_or("").to_string();

                    let channel_msg = ChannelMessage {
                        id: Uuid::new_v4().to_string(),
                        sender: channel_id,
                        content: content.to_string(),
                        channel: "discord".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        attachments,
                    };

                    if tx.send(channel_msg).await.is_err() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn send_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let url = format!("https://discord.com/api/v10/channels/{recipient}/typing");
        self.client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await
            .context("send Discord typing indicator")?;
        Ok(())
    }

    async fn send_media(
        &self,
        attachment: &MediaAttachment,
        recipient: &str,
    ) -> anyhow::Result<()> {
        let url = format!("https://discord.com/api/v10/channels/{recipient}/messages");
        let bytes = match &attachment.data {
            MediaData::Url(media_url) => self
                .client
                .get(media_url)
                .send()
                .await
                .context("download media for Discord upload")?
                .bytes()
                .await
                .context("read media bytes")?
                .to_vec(),
            MediaData::Bytes(b) => b.clone(),
        };
        let filename = attachment
            .filename
            .as_deref()
            .unwrap_or("attachment")
            .to_string();
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename)
            .mime_str(&attachment.mime_type)?;
        let form = reqwest::multipart::Form::new().part("files[0]", part);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .multipart(form)
            .send()
            .await
            .context("send Discord media")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response body: {e}>"));
            anyhow::bail!("Discord media send failed ({status}): {err}");
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.client
            .get("https://discord.com/api/v10/users/@me")
            .header("Authorization", format!("Bot {}", self.bot_token))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_channel_name() {
        let ch = DiscordChannel::new("fake".into(), None, vec![]);
        assert_eq!(ch.name(), "discord");
    }

    #[test]
    fn base64_decode_bot_id() {
        // "MTIzNDU2" decodes to "123456"
        let decoded = base64_decode("MTIzNDU2");
        assert_eq!(decoded, Some("123456".to_string()));
    }

    #[test]
    fn bot_user_id_extraction() {
        // Token format: base64(user_id).timestamp.hmac
        let token = "MTIzNDU2.fake.hmac";
        let id = DiscordChannel::bot_user_id_from_token(token);
        assert_eq!(id, Some("123456".to_string()));
    }

    #[test]
    fn empty_allowlist_denies_everyone() {
        let ch = DiscordChannel::new("fake".into(), None, vec![]);
        assert!(!ch.is_user_allowed("12345"));
        assert!(!ch.is_user_allowed("anyone"));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let ch = DiscordChannel::new("fake".into(), None, vec!["*".into()]);
        assert!(ch.is_user_allowed("12345"));
        assert!(ch.is_user_allowed("anyone"));
    }

    #[test]
    fn specific_allowlist_filters() {
        let ch = DiscordChannel::new("fake".into(), None, vec!["111".into(), "222".into()]);
        assert!(ch.is_user_allowed("111"));
        assert!(ch.is_user_allowed("222"));
        assert!(!ch.is_user_allowed("333"));
        assert!(!ch.is_user_allowed("unknown"));
    }

    #[test]
    fn allowlist_is_exact_match_not_substring() {
        let ch = DiscordChannel::new("fake".into(), None, vec!["111".into()]);
        assert!(!ch.is_user_allowed("1111"));
        assert!(!ch.is_user_allowed("11"));
        assert!(!ch.is_user_allowed("0111"));
    }

    #[test]
    fn allowlist_empty_string_user_id() {
        let ch = DiscordChannel::new("fake".into(), None, vec!["111".into()]);
        assert!(!ch.is_user_allowed(""));
    }

    #[test]
    fn allowlist_with_wildcard_and_specific() {
        let ch = DiscordChannel::new("fake".into(), None, vec!["111".into(), "*".into()]);
        assert!(ch.is_user_allowed("111"));
        assert!(ch.is_user_allowed("anyone_else"));
    }

    #[test]
    fn allowlist_case_sensitive() {
        let ch = DiscordChannel::new("fake".into(), None, vec!["ABC".into()]);
        assert!(ch.is_user_allowed("ABC"));
        assert!(!ch.is_user_allowed("abc"));
        assert!(!ch.is_user_allowed("Abc"));
    }

    #[test]
    fn base64_decode_empty_string() {
        let decoded = base64_decode("");
        assert_eq!(decoded, Some(String::new()));
    }

    #[test]
    fn base64_decode_invalid_chars() {
        let decoded = base64_decode("!!!!");
        assert!(decoded.is_none());
    }

    #[test]
    fn bot_user_id_from_empty_token() {
        let id = DiscordChannel::bot_user_id_from_token("");
        assert_eq!(id, Some(String::new()));
    }

    #[test]
    fn gateway_intents_include_direct_messages() {
        // GUILDS(1) | GUILD_MESSAGES(512) | DIRECT_MESSAGES(4096) | MESSAGE_CONTENT(32768) = 37377
        let intents: u64 = 37377;
        assert_ne!(intents & 1, 0, "GUILDS");
        assert_ne!(intents & 512, 0, "GUILD_MESSAGES");
        assert_ne!(intents & 4096, 0, "DIRECT_MESSAGES");
        assert_ne!(intents & 32768, 0, "MESSAGE_CONTENT");
    }

    #[test]
    fn parse_attachments_from_discord_payload() {
        let d = serde_json::json!({
            "attachments": [
                {
                    "id": "123",
                    "filename": "image.png",
                    "content_type": "image/png",
                    "url": "https://cdn.discordapp.com/attachments/1/2/image.png",
                    "size": 12345
                },
                {
                    "id": "456",
                    "filename": "doc.pdf",
                    "url": "https://cdn.discordapp.com/attachments/1/2/doc.pdf",
                    "size": 999
                }
            ]
        });

        let attachments = DiscordChannel::parse_attachments(&d);
        assert_eq!(attachments.len(), 2);

        assert_eq!(attachments[0].mime_type, "image/png");
        assert_eq!(attachments[0].filename.as_deref(), Some("image.png"));
        assert!(matches!(&attachments[0].data, MediaData::Url(u) if u.contains("image.png")));

        assert_eq!(attachments[1].mime_type, "application/octet-stream");
        assert_eq!(attachments[1].filename.as_deref(), Some("doc.pdf"));
    }

    #[test]
    fn parse_attachments_empty_array() {
        let d = serde_json::json!({ "attachments": [] });
        assert!(DiscordChannel::parse_attachments(&d).is_empty());
    }

    #[test]
    fn parse_attachments_missing_field() {
        let d = serde_json::json!({ "content": "hello" });
        assert!(DiscordChannel::parse_attachments(&d).is_empty());
    }

    #[test]
    fn embed_json_construction() {
        let mut embed = serde_json::json!({ "description": "test body" });
        embed["title"] = serde_json::json!("Test Title");
        embed["color"] = serde_json::json!(0x00FF00);

        let body = serde_json::json!({ "embeds": [embed] });
        let embeds = body["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0]["title"], "Test Title");
        assert_eq!(embeds[0]["description"], "test body");
        assert_eq!(embeds[0]["color"], 0x00FF00);
    }
}
