use crate::config::Config;
use crate::ui::style as ui;

use super::domain::provider_env_var;

pub fn print_welcome_banner() {
    println!("{}", ui::accent(t!("onboard.banner.art")));

    println!("  {}", ui::header(t!("onboard.banner.welcome")));
    println!("  {}", ui::dim(t!("onboard.banner.subtitle")));
    println!();
}

pub fn print_step(current: u8, total: u8, title: &str) {
    println!();
    println!(
        "  {} {}",
        ui::accent(format!("[{current}/{total}]")),
        ui::header(title)
    );
    println!("  {}", ui::dim("─".repeat(50)));
}

pub fn print_bullet(text: &str) {
    println!("  {} {}", ui::cyan("›"), text);
}
#[allow(clippy::too_many_lines)]
pub fn print_summary(config: &Config) {
    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
        || config.channels_config.email.is_some();

    println!();
    println!(
        "  {}",
        ui::cyan("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    );
    println!("  ◆  {}", ui::header(t!("onboard.summary.ready")));
    println!(
        "  {}",
        ui::cyan("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    );
    println!();

    println!("  {}", ui::dim(t!("onboard.summary.config_saved")));
    println!("    {}", ui::value(config.config_path.display()));
    println!();

    println!("  {}", ui::header(t!("onboard.summary.quick_summary")));
    println!(
        "    › {} {}",
        t!("onboard.summary.provider"),
        config.default_provider.as_deref().unwrap_or("openrouter")
    );
    println!(
        "    › {} {}",
        t!("onboard.summary.model"),
        config.default_model.as_deref().unwrap_or("(default)")
    );
    println!(
        "    › {} {:?}",
        t!("onboard.summary.autonomy"),
        config.autonomy.effective_autonomy_level()
    );
    println!(
        "    › {} {} (auto-save: {})",
        t!("onboard.summary.memory"),
        config.memory.backend,
        if config.memory.auto_save { "on" } else { "off" }
    );

    // Channels summary
    let mut channels: Vec<&str> = vec!["CLI"];
    if config.channels_config.telegram.is_some() {
        channels.push("Telegram");
    }
    if config.channels_config.discord.is_some() {
        channels.push("Discord");
    }
    if config.channels_config.slack.is_some() {
        channels.push("Slack");
    }
    if config.channels_config.imessage.is_some() {
        channels.push("iMessage");
    }
    if config.channels_config.matrix.is_some() {
        channels.push("Matrix");
    }
    if config.channels_config.email.is_some() {
        channels.push("Email");
    }
    if config.channels_config.webhook.is_some() {
        channels.push("Webhook");
    }
    println!(
        "    › {} {}",
        t!("onboard.summary.channels"),
        channels.join(", ")
    );

    println!(
        "    › {} {}",
        t!("onboard.summary.api_key"),
        if config.api_key.is_some() {
            ui::value(t!("onboard.summary.api_key_set"))
        } else {
            ui::yellow(t!("onboard.summary.api_key_not_set"))
        }
    );

    // Tunnel
    println!(
        "    › {} {}",
        t!("onboard.summary.tunnel"),
        if config.tunnel.provider == "none" || config.tunnel.provider.is_empty() {
            t!("onboard.summary.tunnel_none").to_string()
        } else {
            config.tunnel.provider.clone()
        }
    );

    // Composio
    println!(
        "    › {} {}",
        t!("onboard.summary.composio"),
        if config.composio.enabled {
            ui::value(t!("onboard.summary.composio_enabled"))
        } else {
            t!("onboard.summary.composio_disabled").to_string()
        }
    );

    // Secrets
    println!(
        "    › {} {}",
        t!("onboard.summary.secrets"),
        if config.secrets.encrypt {
            ui::value(t!("onboard.summary.secrets_encrypted"))
        } else {
            ui::yellow(t!("onboard.summary.secrets_plaintext"))
        }
    );

    // Gateway
    println!(
        "    › {} {}",
        t!("onboard.summary.gateway"),
        if config.gateway.require_pairing {
            t!("onboard.summary.gateway_pairing")
        } else {
            t!("onboard.summary.gateway_no_pairing")
        }
    );

    println!();
    println!("  {}", ui::header(t!("onboard.summary.next_steps")));
    println!();

    let mut step = 1u8;

    if config.api_key.is_none() {
        let env_var = provider_env_var(config.default_provider.as_deref().unwrap_or("openrouter"));
        println!(
            "    {} {}",
            ui::accent(format!("{step}.")),
            t!("onboard.summary.set_api_key")
        );
        println!(
            "       {}",
            ui::yellow(format!("export {env_var}=\"sk-...\""))
        );
        println!();
        step += 1;
    }

    // If channels are configured, show channel start as the primary next step
    if has_channels {
        println!(
            "    {} {} {}",
            ui::accent(format!("{step}.")),
            ui::header(t!("onboard.summary.launch_channels")),
            t!("onboard.summary.launch_channels_hint")
        );
        println!("       {}", ui::yellow("asteroniris channel start"));
        println!();
        step += 1;
    }

    println!(
        "    {} {}",
        ui::accent(format!("{step}.")),
        t!("onboard.summary.send_message")
    );
    println!(
        "       {}",
        ui::yellow("asteroniris agent -m \"Hello, AsteronIris!\"")
    );
    println!();
    step += 1;

    println!(
        "    {} {}",
        ui::accent(format!("{step}.")),
        t!("onboard.summary.interactive_cli")
    );
    println!("       {}", ui::yellow("asteroniris agent"));
    println!();
    step += 1;

    println!(
        "    {} {}",
        ui::accent(format!("{step}.")),
        t!("onboard.summary.check_status")
    );
    println!("       {}", ui::yellow("asteroniris status"));

    println!();
    println!("  ◆ {}", ui::header(t!("onboard.summary.happy_hacking")));
    println!();
}
