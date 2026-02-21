mod attachments;
pub mod chunker;
pub mod cli;
#[cfg(feature = "discord")]
pub mod discord;
#[cfg(not(feature = "discord"))]
pub mod discord {
    use serde::{Deserialize, Serialize};

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
        #[serde(default)]
        pub autonomy_level: Option<crate::security::AutonomyLevel>,
        #[serde(default)]
        pub tool_allowlist: Option<Vec<String>>,
    }
}
#[cfg(feature = "email")]
pub mod email_channel;
// Feature-gate stub: mirrors EmailConfig when "email" feature is disabled.
// MUST stay in sync with src/channels/email_channel.rs EmailConfig.
// Fields: imap_host, imap_port, imap_folder, smtp_host, smtp_port, smtp_tls,
//         username, password, from_address, poll_interval_secs, allowed_senders
#[cfg(not(feature = "email"))]
pub mod email_channel {
    use serde::{Deserialize, Serialize};

    fn default_imap_port() -> u16 {
        993
    }
    fn default_smtp_port() -> u16 {
        587
    }
    fn default_imap_folder() -> String {
        "INBOX".into()
    }
    fn default_poll_interval() -> u64 {
        60
    }
    fn default_true() -> bool {
        true
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EmailConfig {
        pub imap_host: String,
        #[serde(default = "default_imap_port")]
        pub imap_port: u16,
        #[serde(default = "default_imap_folder")]
        pub imap_folder: String,
        pub smtp_host: String,
        #[serde(default = "default_smtp_port")]
        pub smtp_port: u16,
        #[serde(default = "default_true")]
        pub smtp_tls: bool,
        pub username: String,
        pub password: String,
        pub from_address: String,
        #[serde(default = "default_poll_interval")]
        pub poll_interval_secs: u64,
        #[serde(default)]
        pub allowed_senders: Vec<String>,
    }

    impl Default for EmailConfig {
        fn default() -> Self {
            Self {
                imap_host: String::new(),
                imap_port: default_imap_port(),
                imap_folder: default_imap_folder(),
                smtp_host: String::new(),
                smtp_port: default_smtp_port(),
                smtp_tls: true,
                username: String::new(),
                password: String::new(),
                from_address: String::new(),
                poll_interval_secs: default_poll_interval(),
                allowed_senders: Vec::new(),
            }
        }
    }
}
pub mod factory;
mod health;
pub mod imessage;
pub mod ingress_policy;
pub mod irc;
pub mod matrix;
mod message_handler;
pub mod policy;
pub mod prompt_builder;
pub mod runtime;
pub mod slack;
mod startup;
pub mod telegram;
pub mod traits;
pub mod whatsapp;

#[cfg(test)]
mod tests;

pub use cli::CliChannel;
#[cfg(feature = "discord")]
pub use discord::DiscordChannel;
#[cfg(feature = "email")]
pub use email_channel::EmailChannel;
pub use imessage::IMessageChannel;
pub use irc::IrcChannel;
pub use matrix::MatrixChannel;
#[allow(unused_imports)]
pub use prompt_builder::{
    SystemPromptOptions, build_system_prompt, build_system_prompt_with_options,
};
pub use slack::SlackChannel;
pub use startup::{doctor_channels, start_channels};
pub use telegram::TelegramChannel;
pub use traits::Channel;
pub use whatsapp::WhatsAppChannel;

use crate::config::Config;
use anyhow::Result;

pub fn handle_command(command: crate::ChannelCommands, config: &Config) -> Result<()> {
    match command {
        crate::ChannelCommands::Start => {
            anyhow::bail!("Start must be handled in main.rs (requires async runtime)")
        }
        crate::ChannelCommands::Doctor => {
            anyhow::bail!("Doctor must be handled in main.rs (requires async runtime)")
        }
        crate::ChannelCommands::List => {
            println!("{}", t!("channels.list_header"));
            println!("  ✓ {}", t!("channels.cli_always"));
            for (name, configured) in [
                ("Telegram", config.channels_config.telegram.is_some()),
                ("Discord", config.channels_config.discord.is_some()),
                ("Slack", config.channels_config.slack.is_some()),
                ("Webhook", config.channels_config.webhook.is_some()),
                ("iMessage", config.channels_config.imessage.is_some()),
                ("Matrix", config.channels_config.matrix.is_some()),
                ("WhatsApp", config.channels_config.whatsapp.is_some()),
                ("Email", config.channels_config.email.is_some()),
                ("IRC", config.channels_config.irc.is_some()),
            ] {
                println!("  {} {name}", if configured { "✓" } else { "✗" });
            }
            println!("\n{}", t!("channels.to_start"));
            println!("{}", t!("channels.to_check"));
            println!("{}", t!("channels.to_configure"));
            Ok(())
        }
        crate::ChannelCommands::Add {
            channel_type,
            config: _,
        } => {
            anyhow::bail!(
                "Channel type '{channel_type}' — use `asteroniris onboard` to configure channels"
            );
        }
        crate::ChannelCommands::Remove { name } => {
            anyhow::bail!("Remove channel '{name}' — edit ~/.asteroniris/config.toml directly");
        }
    }
}
