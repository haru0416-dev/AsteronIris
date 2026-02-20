use anyhow::Result;
use console::style;
use dialoguer::{Confirm, Input, Select};

use super::super::view::print_bullet;

#[allow(clippy::too_many_lines)]
pub fn setup_tunnel() -> Result<crate::config::TunnelConfig> {
    use crate::config::schema::{
        CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, TailscaleTunnelConfig,
        TunnelConfig,
    };

    print_bullet(&t!("onboard.tunnel.intro"));
    print_bullet(&t!("onboard.tunnel.skip_hint"));
    println!();

    let options = vec![
        t!("onboard.tunnel.skip").to_string(),
        t!("onboard.tunnel.cloudflare").to_string(),
        t!("onboard.tunnel.tailscale").to_string(),
        t!("onboard.tunnel.ngrok").to_string(),
        t!("onboard.tunnel.custom").to_string(),
    ];

    let choice = Select::new()
        .with_prompt(format!("  {}", t!("onboard.tunnel.select_prompt")))
        .items(&options)
        .default(0)
        .interact()?;

    let config = match choice {
        1 => {
            println!();
            print_bullet(&t!("onboard.tunnel.cloudflare_token_hint"));
            let token: String = Input::new()
                .with_prompt(format!(
                    "  {}",
                    t!("onboard.tunnel.cloudflare_token_prompt")
                ))
                .interact_text()?;
            if token.trim().is_empty() {
                println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
                TunnelConfig::default()
            } else {
                println!(
                    "  {} Tunnel: {}",
                    style("✓").green().bold(),
                    style("Cloudflare").green()
                );
                TunnelConfig {
                    provider: "cloudflare".into(),
                    cloudflare: Some(CloudflareTunnelConfig { token }),
                    ..TunnelConfig::default()
                }
            }
        }
        2 => {
            println!();
            print_bullet(&t!("onboard.tunnel.tailscale_hint"));
            let funnel = Confirm::new()
                .with_prompt(format!(
                    "  {}",
                    t!("onboard.tunnel.tailscale_funnel_prompt")
                ))
                .default(false)
                .interact()?;
            println!(
                "  {} Tunnel: {} ({})",
                style("✓").green().bold(),
                style("Tailscale").green(),
                if funnel {
                    t!("onboard.tunnel.tailscale_funnel_public")
                } else {
                    t!("onboard.tunnel.tailscale_serve_tailnet")
                }
            );
            TunnelConfig {
                provider: "tailscale".into(),
                tailscale: Some(TailscaleTunnelConfig {
                    funnel,
                    hostname: None,
                }),
                ..TunnelConfig::default()
            }
        }
        3 => {
            println!();
            print_bullet(&t!("onboard.tunnel.ngrok_hint"));
            let auth_token: String = Input::new()
                .with_prompt(format!("  {}", t!("onboard.tunnel.ngrok_token_prompt")))
                .interact_text()?;
            if auth_token.trim().is_empty() {
                println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
                TunnelConfig::default()
            } else {
                let domain: String = Input::new()
                    .with_prompt(format!("  {}", t!("onboard.tunnel.ngrok_domain_prompt")))
                    .allow_empty(true)
                    .interact_text()?;
                println!(
                    "  {} Tunnel: {}",
                    style("✓").green().bold(),
                    style("ngrok").green()
                );
                TunnelConfig {
                    provider: "ngrok".into(),
                    ngrok: Some(NgrokTunnelConfig {
                        auth_token,
                        domain: if domain.is_empty() {
                            None
                        } else {
                            Some(domain)
                        },
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        4 => {
            println!();
            print_bullet(&t!("onboard.tunnel.custom_hint"));
            print_bullet(&t!("onboard.tunnel.custom_placeholder_hint"));
            print_bullet(&t!("onboard.tunnel.custom_example"));
            let cmd: String = Input::new()
                .with_prompt(format!("  {}", t!("onboard.tunnel.custom_prompt")))
                .interact_text()?;
            if cmd.trim().is_empty() {
                println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
                TunnelConfig::default()
            } else {
                println!(
                    "  {} Tunnel: {} ({})",
                    style("✓").green().bold(),
                    style(t!("onboard.tunnel.confirm_custom")).green(),
                    style(&cmd).dim()
                );
                TunnelConfig {
                    provider: "custom".into(),
                    custom: Some(CustomTunnelConfig {
                        start_command: cmd,
                        health_url: None,
                        url_pattern: None,
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        _ => {
            println!(
                "  {} Tunnel: {}",
                style("✓").green().bold(),
                style(t!("onboard.tunnel.confirm_none")).dim()
            );
            TunnelConfig::default()
        }
    };

    Ok(config)
}
