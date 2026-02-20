pub mod chunker;
pub mod cli;
pub mod discord;
#[cfg(feature = "email")]
pub mod email_channel;
pub mod factory;
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
pub mod imessage;
pub mod ingress_policy;
pub mod irc;
pub mod matrix;
pub mod prompt_builder;
pub mod runtime;
pub mod slack;
pub mod telegram;
pub mod traits;
pub mod whatsapp;

pub use cli::CliChannel;
pub use discord::DiscordChannel;
#[cfg(feature = "email")]
pub use email_channel::EmailChannel;
pub use imessage::IMessageChannel;
pub use irc::IrcChannel;
pub use matrix::MatrixChannel;
pub use prompt_builder::{
    SystemPromptOptions, build_system_prompt, build_system_prompt_with_options,
};
pub use slack::SlackChannel;
pub use telegram::TelegramChannel;
pub use traits::Channel;
pub use whatsapp::WhatsAppChannel;

use crate::auth::AuthBroker;
use crate::config::Config;
use crate::memory::{self, Memory};
use crate::providers::{self, Provider};
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use ingress_policy::{
    apply_external_ingress_policy, channel_autosave_entity_id, channel_autosave_input,
    channel_runtime_policy_context,
};
use runtime::{channel_backoff_settings, spawn_supervised_listener};
use std::sync::Arc;
use std::time::Duration;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelHealthState {
    Healthy,
    Unhealthy,
    Timeout,
}

fn classify_health_result(
    result: &std::result::Result<bool, tokio::time::error::Elapsed>,
) -> ChannelHealthState {
    match result {
        Ok(true) => ChannelHealthState::Healthy,
        Ok(false) => ChannelHealthState::Unhealthy,
        Err(_) => ChannelHealthState::Timeout,
    }
}

pub async fn doctor_channels(config: Config) -> Result<()> {
    let channels = factory::build_channels(&config.channels_config);

    if channels.is_empty() {
        println!("{}", t!("channels.no_channels_doctor"));
        return Ok(());
    }

    println!("◆ {}", t!("channels.doctor_title"));
    println!();

    let mut healthy = 0_u32;
    let mut unhealthy = 0_u32;
    let mut timeout = 0_u32;

    for (name, channel) in channels {
        let result = tokio::time::timeout(Duration::from_secs(10), channel.health_check()).await;
        let state = classify_health_result(&result);

        match state {
            ChannelHealthState::Healthy => {
                healthy += 1;
                println!("  ✓ {name:<9} {}", t!("channels.healthy"));
            }
            ChannelHealthState::Unhealthy => {
                unhealthy += 1;
                println!("  ✗ {name:<9} {}", t!("channels.unhealthy"));
            }
            ChannelHealthState::Timeout => {
                timeout += 1;
                println!("  ! {name:<9} {}", t!("channels.timed_out"));
            }
        }
    }

    if config.channels_config.webhook.is_some() {
        println!("  › {}", t!("channels.webhook_hint"));
    }

    println!();
    println!(
        "{}",
        t!(
            "channels.doctor_summary",
            healthy = healthy,
            unhealthy = unhealthy,
            timeout = timeout
        )
    );
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub async fn start_channels(config: Config) -> Result<()> {
    let auth_broker = AuthBroker::load_or_init(&config)?;

    let provider: Arc<dyn Provider> =
        Arc::from(providers::create_resilient_provider_with_oauth_recovery(
            &config,
            config.default_provider.as_deref().unwrap_or("openrouter"),
            &config.reliability,
            |name| auth_broker.resolve_provider_api_key(name),
        )?);

    if let Err(e) = provider.warmup().await {
        tracing::warn!("Provider warmup failed (non-fatal): {e}");
    }

    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
    let temperature = config.default_temperature;
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        memory_api_key.as_deref(),
    )?);

    let workspace = config.workspace_dir.clone();
    let skills = crate::skills::load_skills(&workspace);

    let tool_descs = crate::tools::tool_descriptions(config.browser.enabled, false);

    let system_prompt = build_system_prompt(&workspace, &model, &tool_descs, &skills);

    if !skills.is_empty() {
        println!(
            "  › {} {}",
            t!("channels.skills"),
            skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let channels: Vec<Arc<dyn Channel>> = factory::build_channels(&config.channels_config)
        .into_iter()
        .map(|(_, ch)| ch)
        .collect();

    if channels.is_empty() {
        println!("{}", t!("channels.no_channels"));
        return Ok(());
    }

    println!("◆ {}", t!("channels.server_title"));
    println!("  › {} {model}", t!("channels.model"));
    println!(
        "  › {} {} (auto-save: {})",
        t!("channels.memory"),
        config.memory.backend,
        if config.memory.auto_save { "on" } else { "off" }
    );
    println!(
        "  › {} {}",
        t!("channels.channels"),
        channels
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!("  {}", t!("channels.listening"));
    println!();

    crate::health::mark_component_ok("channels");

    let (initial_backoff_secs, max_backoff_secs) = channel_backoff_settings(&config.reliability);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<traits::ChannelMessage>(100);

    let mut handles = Vec::with_capacity(channels.len());
    for ch in &channels {
        handles.push(spawn_supervised_listener(
            ch.clone(),
            tx.clone(),
            initial_backoff_secs,
            max_backoff_secs,
        ));
    }
    drop(tx);

    while let Some(msg) = rx.recv().await {
        println!(
            "  › {}",
            t!(
                "channels.message_in",
                channel = msg.channel,
                sender = msg.sender,
                content = truncate_with_ellipsis(&msg.content, 80)
            )
        );

        let source = format!("channel:{}", msg.channel);
        let ingress = apply_external_ingress_policy(&source, &msg.content);

        if config.memory.auto_save {
            let policy_context = channel_runtime_policy_context();
            if let Err(error) = policy_context.enforce_recall_scope(channel_autosave_entity_id()) {
                tracing::warn!(error, "channel autosave skipped due to policy context");
            } else {
                let _ = mem
                    .append_event(channel_autosave_input(
                        &msg.channel,
                        &msg.sender,
                        ingress.persisted_summary.clone(),
                    ))
                    .await;
            }
        }

        if ingress.blocked {
            tracing::warn!(
                source,
                "blocked high-risk external content at channel ingress"
            );
            for ch in &channels {
                if ch.name() == msg.channel {
                    let _ = ch
                        .send_chunked(
                            "⚠️ External content was blocked by safety policy.",
                            &msg.sender,
                        )
                        .await;
                    break;
                }
            }
            continue;
        }

        match provider
            .chat_with_system(
                Some(&system_prompt),
                &ingress.model_input,
                &model,
                temperature,
            )
            .await
        {
            Ok(response) => {
                println!(
                    "  › {} {}",
                    t!("channels.reply"),
                    truncate_with_ellipsis(&response, 80)
                );
                for ch in &channels {
                    if ch.name() == msg.channel {
                        if let Err(e) = ch.send_chunked(&response, &msg.sender).await {
                            eprintln!(
                                "  ✗ {}",
                                t!("channels.reply_fail", channel = ch.name(), error = e)
                            );
                        }
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("  ✗ {}", t!("channels.llm_error", error = e));
                for ch in &channels {
                    if ch.name() == msg.channel {
                        let _ = ch.send_chunked(&format!("! Error: {e}"), &msg.sender).await;
                        break;
                    }
                }
            }
        }
    }

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_health_ok_true() {
        let state = classify_health_result(&Ok(true));
        assert_eq!(state, ChannelHealthState::Healthy);
    }

    #[test]
    fn classify_health_ok_false() {
        let state = classify_health_result(&Ok(false));
        assert_eq!(state, ChannelHealthState::Unhealthy);
    }

    #[tokio::test]
    async fn classify_health_timeout() {
        let result = tokio::time::timeout(Duration::from_millis(1), async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            true
        })
        .await;
        let state = classify_health_result(&result);
        assert_eq!(state, ChannelHealthState::Timeout);
    }
}
