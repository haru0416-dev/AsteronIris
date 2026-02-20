use crate::config::{ComposioConfig, SecretsConfig};
use crate::ui::style as ui;
use anyhow::Result;
use dialoguer::{Confirm, Input, Select};

use super::super::view::print_bullet;

pub fn setup_tool_mode() -> Result<(ComposioConfig, SecretsConfig)> {
    print_bullet(&t!("onboard.tool_mode.intro"));
    print_bullet(&t!("onboard.tool_mode.later_hint"));
    println!();

    let options = vec![
        t!("onboard.tool_mode.sovereign").to_string(),
        t!("onboard.tool_mode.composio").to_string(),
    ];

    let choice = Select::new()
        .with_prompt(format!("  {}", t!("onboard.tool_mode.select_prompt")))
        .items(&options)
        .default(0)
        .interact()?;

    let composio_config = if choice == 1 {
        println!();
        println!(
            "  {} {}",
            ui::header(t!("onboard.tool_mode.composio_title")),
            ui::dim(format!("— {}", t!("onboard.tool_mode.composio_subtitle")))
        );
        print_bullet(&t!("onboard.tool_mode.composio_url_hint"));
        print_bullet(&t!("onboard.tool_mode.composio_desc"));
        println!();

        let api_key: String = Input::new()
            .with_prompt(format!("  {}", t!("onboard.tool_mode.composio_key_prompt")))
            .allow_empty(true)
            .interact_text()?;

        if api_key.trim().is_empty() {
            println!(
                "  {} {}",
                ui::dim("→"),
                t!("onboard.tool_mode.composio_skipped")
            );
            ComposioConfig::default()
        } else {
            println!(
                "  {} {}",
                ui::success("✓"),
                t!("onboard.tool_mode.composio_confirm")
            );
            ComposioConfig {
                enabled: true,
                api_key: Some(api_key),
                ..ComposioConfig::default()
            }
        }
    } else {
        println!(
            "  {} {}",
            ui::success("✓"),
            t!("onboard.tool_mode.sovereign_confirm")
        );
        ComposioConfig::default()
    };

    // ── Encrypted secrets ──
    println!();
    print_bullet(&t!("onboard.tool_mode.encrypt_intro"));
    print_bullet(&t!("onboard.tool_mode.encrypt_desc"));

    let encrypt = Confirm::new()
        .with_prompt(format!("  {}", t!("onboard.tool_mode.encrypt_prompt")))
        .default(true)
        .interact()?;

    let secrets_config = SecretsConfig { encrypt };

    if encrypt {
        println!(
            "  {} {}",
            ui::success("✓"),
            t!("onboard.tool_mode.encrypt_on")
        );
    } else {
        println!(
            "  {} {}",
            ui::success("✓"),
            t!("onboard.tool_mode.encrypt_off")
        );
    }

    Ok((composio_config, secrets_config))
}
