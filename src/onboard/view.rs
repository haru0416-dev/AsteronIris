use crate::config::Config;
use console::style;

use super::domain::provider_env_var;

pub fn print_welcome_banner() {
    println!("{}", style(t!("onboard.banner.art")).cyan().bold());

    println!("  {}", style(t!("onboard.banner.welcome")).white().bold());
    println!("  {}", style(t!("onboard.banner.subtitle")).dim());
    println!();
}

pub fn print_step(current: u8, total: u8, title: &str) {
    println!();
    println!(
        "  {} {}",
        style(format!("[{current}/{total}]")).cyan().bold(),
        style(title).white().bold()
    );
    println!("  {}", style("─".repeat(50)).dim());
}

pub fn print_bullet(text: &str) {
    println!("  {} {}", style("›").cyan(), text);
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
        style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").cyan()
    );
    println!("  ◆  {}", style(t!("onboard.summary.ready")).white().bold());
    println!(
        "  {}",
        style("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━").cyan()
    );
    println!();

    println!("  {}", style(t!("onboard.summary.config_saved")).dim());
    println!("    {}", style(config.config_path.display()).green());
    println!();

    println!(
        "  {}",
        style(t!("onboard.summary.quick_summary")).white().bold()
    );
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
        config.autonomy.level
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
            style(t!("onboard.summary.api_key_set")).green().to_string()
        } else {
            style(t!("onboard.summary.api_key_not_set"))
                .yellow()
                .to_string()
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
            style(t!("onboard.summary.composio_enabled"))
                .green()
                .to_string()
        } else {
            t!("onboard.summary.composio_disabled").to_string()
        }
    );

    // Secrets
    println!(
        "    › {} {}",
        t!("onboard.summary.secrets"),
        if config.secrets.encrypt {
            style(t!("onboard.summary.secrets_encrypted"))
                .green()
                .to_string()
        } else {
            style(t!("onboard.summary.secrets_plaintext"))
                .yellow()
                .to_string()
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
    println!(
        "  {}",
        style(t!("onboard.summary.next_steps")).white().bold()
    );
    println!();

    let mut step = 1u8;

    if config.api_key.is_none() {
        let env_var = provider_env_var(config.default_provider.as_deref().unwrap_or("openrouter"));
        println!(
            "    {} {}",
            style(format!("{step}.")).cyan().bold(),
            t!("onboard.summary.set_api_key")
        );
        println!(
            "       {}",
            style(format!("export {env_var}=\"sk-...\"")).yellow()
        );
        println!();
        step += 1;
    }

    // If channels are configured, show channel start as the primary next step
    if has_channels {
        println!(
            "    {} {} {}",
            style(format!("{step}.")).cyan().bold(),
            style(t!("onboard.summary.launch_channels")).white().bold(),
            t!("onboard.summary.launch_channels_hint")
        );
        println!("       {}", style("asteroniris channel start").yellow());
        println!();
        step += 1;
    }

    println!(
        "    {} {}",
        style(format!("{step}.")).cyan().bold(),
        t!("onboard.summary.send_message")
    );
    println!(
        "       {}",
        style("asteroniris agent -m \"Hello, AsteronIris!\"").yellow()
    );
    println!();
    step += 1;

    println!(
        "    {} {}",
        style(format!("{step}.")).cyan().bold(),
        t!("onboard.summary.interactive_cli")
    );
    println!("       {}", style("asteroniris agent").yellow());
    println!();
    step += 1;

    println!(
        "    {} {}",
        style(format!("{step}.")).cyan().bold(),
        t!("onboard.summary.check_status")
    );
    println!("       {}", style("asteroniris status").yellow());

    println!();
    println!(
        "  ◆ {}",
        style(t!("onboard.summary.happy_hacking")).white().bold()
    );
    println!();
}
