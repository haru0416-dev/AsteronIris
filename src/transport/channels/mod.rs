mod attachments;
pub mod chunker;
pub mod cli;
#[cfg(feature = "discord")]
pub mod discord;
#[cfg(feature = "email")]
pub mod email;
pub mod factory;
mod health;
#[cfg(feature = "imessage")]
pub mod imessage;
pub mod ingress_policy;
#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "matrix")]
pub mod matrix;
mod message_handler;
pub mod policy;
pub mod prompt_builder;
pub mod runtime;
#[cfg(feature = "slack")]
pub mod slack;
mod startup;
#[cfg(feature = "telegram")]
pub mod telegram;
pub mod traits;
#[cfg(feature = "whatsapp")]
pub mod whatsapp;

#[cfg(test)]
mod tests;

pub use cli::CliChannel;
#[cfg(feature = "discord")]
pub use discord::DiscordChannel;
#[cfg(feature = "email")]
pub use email::EmailChannel;
#[cfg(feature = "imessage")]
pub use imessage::IMessageChannel;
#[cfg(feature = "irc")]
pub use irc::{IrcChannel, IrcChannelConfig};
#[cfg(feature = "matrix")]
pub use matrix::MatrixChannel;
#[allow(unused_imports)]
pub use prompt_builder::{
    SystemPromptOptions, build_system_prompt, build_system_prompt_with_options,
};
#[cfg(feature = "slack")]
pub use slack::SlackChannel;
pub use startup::{doctor_channels, start_channels};
#[cfg(feature = "telegram")]
pub use telegram::TelegramChannel;
pub use traits::Channel;
#[cfg(feature = "whatsapp")]
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
            for (name, configured) in config.channels_config.configured_channel_flags() {
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
