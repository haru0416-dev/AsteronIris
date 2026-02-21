use crate::security::AutonomyLevel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    #[serde(default = "default_cli_enabled")]
    pub cli: bool,
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub webhook: Option<WebhookConfig>,
    pub imessage: Option<IMessageConfig>,
    pub matrix: Option<MatrixConfig>,
    pub whatsapp: Option<WhatsAppConfig>,
    pub email: Option<EmailConfig>,
    pub irc: Option<IrcConfig>,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            cli: true,
            telegram: None,
            discord: None,
            slack: None,
            webhook: None,
            imessage: None,
            matrix: None,
            whatsapp: None,
            email: None,
            irc: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_users: Vec<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub bot_token: String,
    #[serde(default)]
    pub application_id: Option<String>,
    pub guild_id: Option<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub intents: Option<u64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub activity_type: Option<u8>,
    #[serde(default)]
    pub activity_name: Option<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: Option<String>,
    pub channel_id: Option<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub port: u16,
    pub secret: Option<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMessageConfig {
    pub allowed_contacts: Vec<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub access_token: String,
    pub room_id: String,
    pub allowed_users: Vec<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    /// Access token from Meta Business Suite
    pub access_token: String,
    /// Phone number ID from Meta Business API
    pub phone_number_id: String,
    /// Webhook verify token (you define this, Meta sends it back for verification)
    pub verify_token: String,
    /// App secret for webhook signature verification (X-Hub-Signature-256)
    #[serde(default)]
    pub app_secret: Option<String>,
    /// Allowed phone numbers (E.164 format: +1234567890) or "*" for all
    #[serde(default)]
    pub allowed_numbers: Vec<String>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrcConfig {
    /// IRC server hostname
    pub server: String,
    /// IRC server port (default: 6697 for TLS)
    #[serde(default = "default_irc_port")]
    pub port: u16,
    /// Bot nickname
    pub nickname: String,
    /// Username (defaults to nickname if not set)
    pub username: Option<String>,
    /// Channels to join on connect
    #[serde(default)]
    pub channels: Vec<String>,
    /// Allowed nicknames (case-insensitive) or "*" for all
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// Server password (for bouncers like ZNC)
    pub server_password: Option<String>,
    /// `NickServ` IDENTIFY password
    pub nickserv_password: Option<String>,
    /// SASL PLAIN password (`IRCv3`)
    pub sasl_password: Option<String>,
    /// Verify TLS certificate (default: true)
    pub verify_tls: Option<bool>,
    /// Per-channel autonomy level override. Effective level = min(global, channel).
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    /// Per-channel tool allowlist. None = all tools permitted.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub imap_host: String,
    #[serde(default = "default_email_imap_port")]
    pub imap_port: u16,
    #[serde(default = "default_email_imap_folder")]
    pub imap_folder: String,
    pub smtp_host: String,
    #[serde(default = "default_email_smtp_port")]
    pub smtp_port: u16,
    #[serde(default = "default_true")]
    pub smtp_tls: bool,
    pub username: String,
    pub password: String,
    pub from_address: String,
    #[serde(default = "default_email_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub allowed_senders: Vec<String>,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            imap_host: String::new(),
            imap_port: default_email_imap_port(),
            imap_folder: default_email_imap_folder(),
            smtp_host: String::new(),
            smtp_port: default_email_smtp_port(),
            smtp_tls: true,
            username: String::new(),
            password: String::new(),
            from_address: String::new(),
            poll_interval_secs: default_email_poll_interval(),
            allowed_senders: Vec::new(),
        }
    }
}

fn deserialize_autonomy_level_opt<'de, D>(
    deserializer: D,
) -> Result<Option<AutonomyLevel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    value
        .map(|level| match level.as_str() {
            "read_only" | "readonly" => Ok(AutonomyLevel::ReadOnly),
            "supervised" => Ok(AutonomyLevel::Supervised),
            "full" => Ok(AutonomyLevel::Full),
            _ => Err(serde::de::Error::unknown_variant(
                &level,
                &["read_only", "readonly", "supervised", "full"],
            )),
        })
        .transpose()
}

fn default_irc_port() -> u16 {
    6697
}

fn default_email_imap_port() -> u16 {
    993
}

fn default_email_smtp_port() -> u16 {
    587
}

fn default_email_imap_folder() -> String {
    "INBOX".into()
}

fn default_email_poll_interval() -> u64 {
    60
}

fn default_true() -> bool {
    true
}

fn default_cli_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_configs_deserialize_without_policy_fields() {
        let telegram: TelegramConfig =
            serde_json::from_str(r#"{"bot_token":"token","allowed_users":["u"]}"#).unwrap();
        assert!(telegram.autonomy_level.is_none());
        assert!(telegram.tool_allowlist.is_none());

        let discord: DiscordConfig =
            serde_json::from_str(r#"{"bot_token":"token","guild_id":null,"allowed_users":[]}"#)
                .unwrap();
        assert!(discord.autonomy_level.is_none());
        assert!(discord.tool_allowlist.is_none());

        let slack: SlackConfig = serde_json::from_str(
            r#"{"bot_token":"token","app_token":null,"channel_id":null,"allowed_users":[]}"#,
        )
        .unwrap();
        assert!(slack.autonomy_level.is_none());
        assert!(slack.tool_allowlist.is_none());

        let webhook: WebhookConfig =
            serde_json::from_str(r#"{"port":8080,"secret":null}"#).unwrap();
        assert!(webhook.autonomy_level.is_none());
        assert!(webhook.tool_allowlist.is_none());

        let imessage: IMessageConfig =
            serde_json::from_str(r#"{"allowed_contacts":["*"]}"#).unwrap();
        assert!(imessage.autonomy_level.is_none());
        assert!(imessage.tool_allowlist.is_none());

        let matrix: MatrixConfig = serde_json::from_str(
            r#"{"homeserver":"https://example.org","access_token":"token","room_id":"!r:example.org","allowed_users":["*"]}"#,
        )
        .unwrap();
        assert!(matrix.autonomy_level.is_none());
        assert!(matrix.tool_allowlist.is_none());

        let whatsapp: WhatsAppConfig = serde_json::from_str(
            r#"{"access_token":"token","phone_number_id":"id","verify_token":"verify","allowed_numbers":["*"],"app_secret":null}"#,
        )
        .unwrap();
        assert!(whatsapp.autonomy_level.is_none());
        assert!(whatsapp.tool_allowlist.is_none());

        let irc: IrcConfig = serde_json::from_str(
            r#"{"server":"irc.example.com","nickname":"bot","port":6697,"username":null,"channels":[],"allowed_users":[],"server_password":null,"nickserv_password":null,"sasl_password":null,"verify_tls":null}"#,
        )
        .unwrap();
        assert!(irc.autonomy_level.is_none());
        assert!(irc.tool_allowlist.is_none());
    }

    #[test]
    fn channel_config_deserializes_policy_fields() {
        let telegram: TelegramConfig = serde_json::from_str(
            r#"{"bot_token":"token","allowed_users":["u"],"autonomy_level":"read_only","tool_allowlist":["file_read"]}"#,
        )
        .unwrap();

        assert_eq!(telegram.autonomy_level, Some(AutonomyLevel::ReadOnly));
        assert_eq!(telegram.tool_allowlist, Some(vec!["file_read".to_string()]));
    }
}
