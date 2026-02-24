use crate::config::Config;
use crate::plugins::integrations::{IntegrationStatus, registry};
use crate::ui::style as ui;
use anyhow::Result;

/// Show information about a specific integration.
pub fn show_integration_info(config: &Config, name: &str) -> Result<()> {
    let entries = registry::all_integrations();
    let name_lower = name.to_lowercase();

    let Some(entry) = entries.iter().find(|e| e.name.to_lowercase() == name_lower) else {
        anyhow::bail!(
            "Unknown integration: {name}. Check README for supported integrations or run `asteroniris onboard --interactive` to configure channels/providers."
        );
    };

    let status = (entry.status_fn)(config);
    let (icon, label) = match status {
        IntegrationStatus::Active => (">>", "Active"),
        IntegrationStatus::Available => ("--", "Available"),
        IntegrationStatus::ComingSoon => ("..", "Coming Soon"),
    };

    println!();
    println!(
        "  {} {} — {}",
        icon,
        ui::header(entry.name),
        entry.description
    );
    println!("  Category: {}", entry.category.label());
    println!("  Status:   {label}");
    println!();

    // Show setup hints based on integration
    match entry.name {
        "Telegram" => {
            println!("  Setup:");
            println!("    1. Message @BotFather on Telegram");
            println!("    2. Create a bot and copy the token");
            println!("    3. Run: asteroniris onboard");
            println!("    4. Start: asteroniris channel start");
        }
        "Discord" => {
            println!("  Setup:");
            println!("    1. Go to https://discord.com/developers/applications");
            println!("    2. Create app -> Bot -> Copy token");
            println!("    3. Enable MESSAGE CONTENT intent");
            println!("    4. Run: asteroniris onboard");
        }
        "Slack" => {
            println!("  Setup:");
            println!("    1. Go to https://api.slack.com/apps");
            println!("    2. Create app -> Bot Token Scopes -> Install");
            println!("    3. Run: asteroniris onboard");
        }
        "OpenRouter" => {
            println!("  Setup:");
            println!("    1. Get API key at https://openrouter.ai/keys");
            println!("    2. Run: asteroniris onboard");
            println!("    Access 200+ models with one key.");
        }
        "Ollama" => {
            println!("  Setup:");
            println!("    1. Install: brew install ollama");
            println!("    2. Pull a model: ollama pull llama3");
            println!("    3. Set provider to 'ollama' in config.toml");
        }
        "iMessage" => {
            println!("  Setup (macOS only):");
            println!("    Uses AppleScript bridge to send/receive iMessages.");
            println!("    Requires Full Disk Access in System Settings -> Privacy.");
        }
        "GitHub" => {
            println!("  Setup:");
            println!("    1. Create a personal access token at https://github.com/settings/tokens");
            println!("    2. Add to config: [integrations.github] token = \"ghp_...\"");
        }
        "Browser" => {
            println!("  Built-in:");
            println!("    AsteronIris can control Chrome/Chromium for web tasks.");
            println!("    Uses headless browser automation.");
        }
        "Cron" => {
            println!("  Built-in:");
            println!("    Schedule tasks in ~/.asteroniris/workspace/cron/");
            println!("    Run: asteroniris cron list");
        }
        "Webhooks" => {
            println!("  Built-in:");
            println!("    HTTP endpoint for external triggers.");
            println!("    Run: asteroniris gateway");
        }
        _ => {
            if status == IntegrationStatus::ComingSoon {
                println!("  This integration is planned. Stay tuned!");
                println!("  Track progress: https://github.com/haru0416-dev/AsteronIris");
            }
        }
    }

    println!();
    Ok(())
}
