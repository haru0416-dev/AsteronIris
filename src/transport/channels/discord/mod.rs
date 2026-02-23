pub mod commands;
pub mod gateway;
pub mod http_client;
pub mod types;

use crate::config::schema::DiscordConfig;
use crate::transport::channels::attachments::media_attachment_url;
use crate::transport::channels::policy::{AllowlistMatch, is_allowed_user};
use crate::transport::channels::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};
use anyhow::Context;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use self::commands::{build_default_commands, defer_interaction, extract_command_input};
use self::gateway::{DiscordGateway, DiscordGatewayState, GatewayEvent};
use self::http_client::DiscordHttpClient;
use self::types::{DEFAULT_INTENTS, InteractionType, MAX_MESSAGE_LENGTH};

pub struct DiscordChannel {
    http: DiscordHttpClient,
    gateway_state: Arc<DiscordGatewayState>,
    config: DiscordConfig,
    bot_user_id: std::sync::Mutex<Option<String>>,
}

struct MessageCreateParams<'a> {
    tx: &'a tokio::sync::mpsc::Sender<ChannelMessage>,
    channel_id: &'a str,
    author_id: &'a str,
    author_is_bot: bool,
    content: String,
    guild_id: Option<&'a str>,
    thread_id: Option<String>,
    message_id: &'a str,
    attachments: &'a [gateway::RawAttachment],
}

struct InteractionCreateParams<'a> {
    tx: &'a tokio::sync::mpsc::Sender<ChannelMessage>,
    interaction_id: &'a str,
    interaction_token: &'a str,
    interaction_type: u64,
    channel_id: &'a str,
    user_id: &'a str,
    guild_id: Option<&'a str>,
    data: &'a serde_json::Value,
}

impl DiscordChannel {
    pub fn new(config: DiscordConfig) -> Self {
        Self {
            http: DiscordHttpClient::new(&config.bot_token),
            gateway_state: Arc::new(DiscordGatewayState::default()),
            config,
            bot_user_id: std::sync::Mutex::new(None),
        }
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        is_allowed_user(&self.config.allowed_users, user_id, AllowlistMatch::Exact)
    }

    fn intents(&self) -> u64 {
        self.config.intents.unwrap_or(DEFAULT_INTENTS)
    }

    fn build_presence(&self) -> Option<serde_json::Value> {
        let status = self.config.status.as_deref().unwrap_or("online");
        let activity_name = self.config.activity_name.as_deref()?;
        let activity_type = self.config.activity_type.unwrap_or(0);

        Some(serde_json::json!({
            "status": status,
            "activities": [{
                "name": activity_name,
                "type": activity_type,
            }],
            "since": null,
            "afk": false,
        }))
    }

    fn matches_guild_filter(&self, guild_id: Option<&str>) -> bool {
        match &self.config.guild_id {
            Some(gid) => guild_id.is_some_and(|g| g == gid),
            None => true,
        }
    }

    fn set_bot_user_id(&self, user_id: &str) {
        if let Ok(mut guard) = self.bot_user_id.lock() {
            *guard = Some(user_id.to_string());
        }
    }

    fn is_bot_user(&self, user_id: &str) -> bool {
        self.bot_user_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .is_some_and(|id| id == user_id)
    }

    fn attachment_to_media(att: &gateway::RawAttachment) -> MediaAttachment {
        media_attachment_url(
            att.url.clone(),
            att.content_type.as_deref(),
            att.filename.clone(),
        )
    }

    async fn handle_gateway_event(
        &self,
        event: GatewayEvent,
        tx: &tokio::sync::mpsc::Sender<ChannelMessage>,
    ) {
        match event {
            GatewayEvent::Ready { user_id, .. } => {
                self.handle_ready(&user_id).await;
            }
            GatewayEvent::MessageCreate {
                channel_id,
                author_id,
                author_is_bot,
                content,
                guild_id,
                thread_id,
                message_id,
                attachments,
            } => {
                self.handle_message_create(MessageCreateParams {
                    tx,
                    channel_id: &channel_id,
                    author_id: &author_id,
                    author_is_bot,
                    content,
                    guild_id: guild_id.as_deref(),
                    thread_id,
                    message_id: &message_id,
                    attachments: &attachments,
                })
                .await;
            }
            GatewayEvent::InteractionCreate {
                interaction_id,
                interaction_token,
                interaction_type,
                channel_id,
                user_id,
                guild_id,
                data,
            } => {
                self.handle_interaction_create(InteractionCreateParams {
                    tx,
                    interaction_id: &interaction_id,
                    interaction_token: &interaction_token,
                    interaction_type,
                    channel_id: &channel_id,
                    user_id: &user_id,
                    guild_id: guild_id.as_deref(),
                    data: &data,
                })
                .await;
            }
            GatewayEvent::ReactionAdd { .. } | GatewayEvent::ReactionRemove { .. } => {
                tracing::trace!("Discord: reaction event received (not yet routed to agent)");
            }
        }
    }

    async fn handle_ready(&self, user_id: &str) {
        self.set_bot_user_id(user_id);
        tracing::info!("Discord: connected as user {user_id}");

        if let Some(app_id) = &self.config.application_id {
            let cmds = build_default_commands();
            if let Err(e) = commands::register_commands(
                &self.http,
                app_id,
                self.config.guild_id.as_deref(),
                &cmds,
            )
            .await
            {
                tracing::warn!("Discord: failed to register slash commands: {e}");
            }
        }
    }

    async fn handle_message_create(&self, params: MessageCreateParams<'_>) {
        let MessageCreateParams {
            tx,
            channel_id,
            author_id,
            author_is_bot,
            content,
            guild_id,
            thread_id,
            message_id,
            attachments,
        } = params;

        if self.is_bot_user(author_id) || author_is_bot {
            return;
        }
        if !self.is_user_allowed(author_id) {
            tracing::warn!("Discord: ignoring message from unauthorized user: {author_id}");
            return;
        }
        if !self.matches_guild_filter(guild_id) {
            return;
        }
        if content.is_empty() && attachments.is_empty() {
            return;
        }

        let msg = ChannelMessage {
            id: Uuid::new_v4().to_string(),
            sender: author_id.to_string(),
            content,
            channel: "discord".to_string(),
            conversation_id: Some(channel_id.to_string()),
            thread_id,
            reply_to: None,
            message_id: Some(message_id.to_string()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            attachments: attachments.iter().map(Self::attachment_to_media).collect(),
        };

        if tx.send(msg).await.is_err() {
            tracing::warn!("Discord: channel message receiver dropped");
        }
    }

    async fn handle_interaction_create(&self, params: InteractionCreateParams<'_>) {
        let InteractionCreateParams {
            tx,
            interaction_id,
            interaction_token,
            interaction_type,
            channel_id,
            user_id,
            guild_id,
            data,
        } = params;

        if InteractionType::from_u64(interaction_type) != Some(InteractionType::ApplicationCommand)
        {
            return;
        }
        if !self.is_user_allowed(user_id) {
            return;
        }
        if !self.matches_guild_filter(guild_id) {
            return;
        }

        let Some(input) = extract_command_input(data) else {
            return;
        };

        if let Err(e) = defer_interaction(&self.http, interaction_id, interaction_token).await {
            tracing::warn!("Discord: failed to defer interaction: {e}");
            return;
        }

        let msg = ChannelMessage {
            id: Uuid::new_v4().to_string(),
            sender: user_id.to_string(),
            content: input,
            channel: "discord".to_string(),
            conversation_id: Some(channel_id.to_string()),
            thread_id: None,
            reply_to: None,
            message_id: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            attachments: vec![],
        };

        if tx.send(msg).await.is_err() {
            tracing::warn!("Discord: channel message receiver dropped");
        }
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    fn max_message_length(&self) -> usize {
        MAX_MESSAGE_LENGTH
    }

    async fn send(&self, message: &str, channel_id: &str) -> anyhow::Result<()> {
        self.http
            .send_message(channel_id, message)
            .await
            .map(|_| ())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let gateway = DiscordGateway::new(
            self.config.bot_token.clone(),
            self.intents(),
            Arc::clone(&self.gateway_state),
            self.build_presence(),
        );

        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<GatewayEvent>(100);

        let mut gateway_handle = {
            let http = DiscordHttpClient::new(&self.config.bot_token);
            tokio::spawn(async move { gateway.connect_and_listen(&http, &event_tx).await })
        };

        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    self.handle_gateway_event(event, &tx).await;
                }
                result = &mut gateway_handle => {
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => return Err(e),
                        Err(e) => anyhow::bail!("Discord gateway task panicked: {e}"),
                    }
                    break;
                }
            }
        }

        Ok(())
    }

    async fn send_typing(&self, recipient: &str) -> anyhow::Result<()> {
        self.http.send_typing(recipient).await
    }

    async fn send_media(
        &self,
        attachment: &MediaAttachment,
        recipient: &str,
    ) -> anyhow::Result<()> {
        let bytes = match &attachment.data {
            MediaData::Url(media_url) => reqwest::Client::new()
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
        self.http
            .send_media(recipient, bytes, &filename, &attachment.mime_type)
            .await
    }

    async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        self.http
            .edit_message(channel_id, message_id, content)
            .await
    }

    async fn delete_message(&self, channel_id: &str, message_id: &str) -> anyhow::Result<()> {
        self.http.delete_message(channel_id, message_id).await
    }

    async fn health_check(&self) -> bool {
        self.http.get_current_user().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> DiscordConfig {
        DiscordConfig {
            bot_token: "fake-token".to_string(),
            application_id: None,
            guild_id: None,
            allowed_users: vec![],
            intents: None,
            status: None,
            activity_type: None,
            activity_name: None,
            autonomy_level: None,
            tool_allowlist: None,
        }
    }

    #[test]
    fn discord_channel_name() {
        let ch = DiscordChannel::new(test_config());
        assert_eq!(ch.name(), "discord");
    }

    #[test]
    fn discord_max_message_length() {
        let ch = DiscordChannel::new(test_config());
        assert_eq!(ch.max_message_length(), 2000);
    }

    #[test]
    fn default_intents_used_when_not_configured() {
        let ch = DiscordChannel::new(test_config());
        assert_eq!(ch.intents(), DEFAULT_INTENTS);
    }

    #[test]
    fn custom_intents_used_when_configured() {
        let mut cfg = test_config();
        cfg.intents = Some(12345);
        let ch = DiscordChannel::new(cfg);
        assert_eq!(ch.intents(), 12345);
    }

    #[test]
    fn empty_allowlist_denies_everyone() {
        let ch = DiscordChannel::new(test_config());
        assert!(!ch.is_user_allowed("12345"));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let mut cfg = test_config();
        cfg.allowed_users = vec!["*".into()];
        let ch = DiscordChannel::new(cfg);
        assert!(ch.is_user_allowed("12345"));
        assert!(ch.is_user_allowed("anyone"));
    }

    #[test]
    fn specific_allowlist_filters() {
        let mut cfg = test_config();
        cfg.allowed_users = vec!["111".into(), "222".into()];
        let ch = DiscordChannel::new(cfg);
        assert!(ch.is_user_allowed("111"));
        assert!(ch.is_user_allowed("222"));
        assert!(!ch.is_user_allowed("333"));
    }

    #[test]
    fn guild_filter_none_accepts_all() {
        let ch = DiscordChannel::new(test_config());
        assert!(ch.matches_guild_filter(Some("any-guild")));
        assert!(ch.matches_guild_filter(None));
    }

    #[test]
    fn guild_filter_specific_rejects_mismatch() {
        let mut cfg = test_config();
        cfg.guild_id = Some("my-guild".into());
        let ch = DiscordChannel::new(cfg);
        assert!(ch.matches_guild_filter(Some("my-guild")));
        assert!(!ch.matches_guild_filter(Some("other-guild")));
        assert!(!ch.matches_guild_filter(None));
    }

    #[test]
    fn presence_none_without_activity_name() {
        let ch = DiscordChannel::new(test_config());
        assert!(ch.build_presence().is_none());
    }

    #[test]
    fn presence_built_with_activity_name() {
        let mut cfg = test_config();
        cfg.activity_name = Some("Watching you".into());
        cfg.activity_type = Some(3);
        cfg.status = Some("dnd".into());
        let ch = DiscordChannel::new(cfg);
        let presence = ch.build_presence().expect("should build presence");
        assert_eq!(presence["status"], "dnd");
        assert_eq!(presence["activities"][0]["name"], "Watching you");
        assert_eq!(presence["activities"][0]["type"], 3);
    }

    #[test]
    fn bot_user_id_tracking() {
        let ch = DiscordChannel::new(test_config());
        assert!(!ch.is_bot_user("123"));
        ch.set_bot_user_id("123");
        assert!(ch.is_bot_user("123"));
        assert!(!ch.is_bot_user("456"));
    }

    #[test]
    fn attachment_conversion() {
        let raw = gateway::RawAttachment {
            url: "https://cdn.discordapp.com/file.png".to_string(),
            filename: Some("file.png".to_string()),
            content_type: Some("image/png".to_string()),
        };
        let media = DiscordChannel::attachment_to_media(&raw);
        assert_eq!(media.mime_type, "image/png");
        assert_eq!(media.filename.as_deref(), Some("file.png"));
        assert!(matches!(media.data, MediaData::Url(u) if u.contains("file.png")));
    }

    #[test]
    fn attachment_conversion_default_mime() {
        let raw = gateway::RawAttachment {
            url: "https://cdn.discordapp.com/blob".to_string(),
            filename: None,
            content_type: None,
        };
        let media = DiscordChannel::attachment_to_media(&raw);
        assert_eq!(media.mime_type, "application/octet-stream");
    }
}
