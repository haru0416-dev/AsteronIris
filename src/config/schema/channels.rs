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

impl ChannelsConfig {
    #[must_use]
    pub fn configured_channel_flags(&self) -> [(&'static str, bool); 9] {
        [
            ("Telegram", self.telegram.is_some()),
            ("Discord", self.discord.is_some()),
            ("Slack", self.slack.is_some()),
            ("Webhook", self.webhook.is_some()),
            ("iMessage", self.imessage.is_some()),
            ("Matrix", self.matrix.is_some()),
            ("WhatsApp", self.whatsapp.is_some()),
            ("Email", self.email.is_some()),
            ("IRC", self.irc.is_some()),
        ]
    }

    #[must_use]
    pub fn active_channel_names(&self) -> Vec<&'static str> {
        let mut active = Vec::with_capacity(10);
        active.push("CLI");
        for (name, configured) in self.configured_channel_flags() {
            if configured {
                active.push(name);
            }
        }
        active
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_users: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
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
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
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
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub port: u16,
    pub secret: Option<String>,
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMessageConfig {
    pub allowed_contacts: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub access_token: String,
    pub room_id: String,
    pub allowed_users: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    pub access_token: String,
    pub phone_number_id: String,
    pub verify_token: String,
    #[serde(default)]
    pub app_secret: Option<String>,
    #[serde(default)]
    pub allowed_numbers: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrcConfig {
    pub server: String,
    #[serde(default = "default_irc_port")]
    pub port: u16,
    pub nickname: String,
    pub username: Option<String>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    pub server_password: Option<String>,
    pub nickserv_password: Option<String>,
    pub sasl_password: Option<String>,
    pub verify_tls: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_autonomy_level_opt")]
    pub autonomy_level: Option<AutonomyLevel>,
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
            "read_only" => Ok(AutonomyLevel::ReadOnly),
            "supervised" => Ok(AutonomyLevel::Supervised),
            "full" => Ok(AutonomyLevel::Full),
            _ => Err(serde::de::Error::unknown_variant(
                &level,
                &["read_only", "supervised", "full"],
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
