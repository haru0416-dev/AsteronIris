use crate::config::schema::{IrcConfig, WhatsAppConfig};
use crate::config::{
    ChannelsConfig, DiscordConfig, IMessageConfig, MatrixConfig, SlackConfig, TelegramConfig,
    WebhookConfig,
};
use anyhow::Result;
use console::style;
use dialoguer::{Confirm, Input, Select};

use super::super::view::print_bullet;

#[allow(clippy::too_many_lines)]
pub fn setup_channels() -> Result<ChannelsConfig> {
    print_bullet(&t!("onboard.channels.intro"));
    print_bullet(&t!("onboard.channels.cli_always"));
    println!();

    let mut config = ChannelsConfig {
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
    };

    loop {
        let connected = t!("onboard.channels.connected");
        let configured = t!("onboard.channels.configured");
        let options = vec![
            format!(
                "Telegram   {}",
                if config.telegram.is_some() {
                    format!("✓ {connected}")
                } else {
                    "— connect your bot".into()
                }
            ),
            format!(
                "Discord    {}",
                if config.discord.is_some() {
                    format!("✓ {connected}")
                } else {
                    "— connect your bot".into()
                }
            ),
            format!(
                "Slack      {}",
                if config.slack.is_some() {
                    format!("✓ {connected}")
                } else {
                    "— connect your bot".into()
                }
            ),
            format!(
                "iMessage   {}",
                if config.imessage.is_some() {
                    format!("✓ {configured}")
                } else {
                    "— macOS only".into()
                }
            ),
            format!(
                "Matrix     {}",
                if config.matrix.is_some() {
                    format!("✓ {connected}")
                } else {
                    "— self-hosted chat".into()
                }
            ),
            format!(
                "WhatsApp   {}",
                if config.whatsapp.is_some() {
                    format!("✓ {connected}")
                } else {
                    "— Business Cloud API".into()
                }
            ),
            format!(
                "IRC        {}",
                if config.irc.is_some() {
                    format!("✓ {configured}")
                } else {
                    "— IRC over TLS".into()
                }
            ),
            format!(
                "Webhook    {}",
                if config.webhook.is_some() {
                    format!("✓ {configured}")
                } else {
                    "— HTTP endpoint".into()
                }
            ),
            t!("onboard.channels.done").to_string(),
        ];

        let choice = Select::new()
            .with_prompt(format!("  {}", t!("onboard.channels.select_prompt")))
            .items(&options)
            .default(8)
            .interact()?;

        match choice {
            0 => setup_telegram(&mut config)?,
            1 => setup_discord(&mut config)?,
            2 => setup_slack(&mut config)?,
            3 => setup_imessage(&mut config)?,
            4 => setup_matrix(&mut config)?,
            5 => setup_whatsapp(&mut config)?,
            6 => setup_irc(&mut config)?,
            7 => setup_webhook(&mut config)?,
            _ => break,
        }
        println!();
    }

    // Summary line
    let mut active: Vec<&str> = vec!["CLI"];
    if config.telegram.is_some() {
        active.push("Telegram");
    }
    if config.discord.is_some() {
        active.push("Discord");
    }
    if config.slack.is_some() {
        active.push("Slack");
    }
    if config.imessage.is_some() {
        active.push("iMessage");
    }
    if config.matrix.is_some() {
        active.push("Matrix");
    }
    if config.whatsapp.is_some() {
        active.push("WhatsApp");
    }
    if config.email.is_some() {
        active.push("Email");
    }
    if config.irc.is_some() {
        active.push("IRC");
    }
    if config.webhook.is_some() {
        active.push("Webhook");
    }

    println!(
        "  {} {}",
        style("✓").green().bold(),
        t!(
            "onboard.channels.summary",
            channels = style(active.join(", ")).green()
        )
    );

    Ok(config)
}

fn setup_telegram(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.telegram_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.telegram_subtitle"))).dim()
    );
    print_bullet(&t!("onboard.channels.telegram_step1"));
    print_bullet(&t!("onboard.channels.telegram_step2"));
    print_bullet(&t!("onboard.channels.telegram_step3"));
    println!();

    let token: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.telegram_token_prompt")
        ))
        .interact_text()?;

    if token.trim().is_empty() {
        println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
        return Ok(());
    }

    // Test connection
    print!("  › {}... ", t!("onboard.channels.testing"));
    let client = reqwest::blocking::Client::new();
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    match client.get(&url).send() {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().unwrap_or_default();
            let bot_name = data
                .get("result")
                .and_then(|r| r.get("username"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            println!(
                "\r  ✓ {}        ",
                t!("onboard.channels.test_success", name = bot_name)
            );
        }
        _ => {
            println!("\r  ✗ {}", t!("onboard.channels.test_fail"));
            return Ok(());
        }
    }

    print_bullet(&t!("onboard.channels.telegram_allowlist_hint"));
    print_bullet(&t!("onboard.channels.telegram_allowlist_format"));
    print_bullet(&t!("onboard.channels.telegram_allowlist_star"));

    let users_str: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.telegram_users_prompt")
        ))
        .allow_empty(true)
        .interact_text()?;

    let allowed_users = parse_allowlist(&users_str);

    if allowed_users.is_empty() {
        println!("  ! {}", t!("onboard.channels.telegram_no_users"));
    }

    config.telegram = Some(TelegramConfig {
        bot_token: token,
        allowed_users,
    });

    Ok(())
}

fn setup_discord(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.discord_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.discord_subtitle"))).dim()
    );
    print_bullet(&t!("onboard.channels.discord_step1"));
    print_bullet(&t!("onboard.channels.discord_step2"));
    print_bullet(&t!("onboard.channels.discord_step3"));
    print_bullet(&t!("onboard.channels.discord_step4"));
    println!();

    let token: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.discord_token_prompt")))
        .interact_text()?;

    if token.trim().is_empty() {
        println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
        return Ok(());
    }

    print!("  › {}... ", t!("onboard.channels.testing"));
    let client = reqwest::blocking::Client::new();
    match client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bot {token}"))
        .send()
    {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().unwrap_or_default();
            let bot_name = data
                .get("username")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            println!(
                "\r  ✓ {}        ",
                t!("onboard.channels.test_success", name = bot_name)
            );
        }
        _ => {
            println!("\r  ✗ {}", t!("onboard.channels.test_fail"));
            return Ok(());
        }
    }

    let guild: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.discord_guild_prompt")))
        .allow_empty(true)
        .interact_text()?;

    print_bullet(&t!("onboard.channels.discord_allowlist_hint"));
    print_bullet(&t!("onboard.channels.discord_allowlist_format"));
    print_bullet(&t!("onboard.channels.discord_allowlist_star"));

    let allowed_users_str: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.discord_users_prompt")))
        .allow_empty(true)
        .interact_text()?;

    let allowed_users = parse_allowlist(&allowed_users_str);

    if allowed_users.is_empty() {
        println!("  ! {}", t!("onboard.channels.discord_no_users"));
    }

    config.discord = Some(DiscordConfig {
        bot_token: token,
        guild_id: if guild.is_empty() { None } else { Some(guild) },
        allowed_users,
    });

    Ok(())
}

fn setup_slack(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.slack_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.slack_subtitle"))).dim()
    );
    print_bullet(&t!("onboard.channels.slack_step1"));
    print_bullet(&t!("onboard.channels.slack_step2"));
    print_bullet(&t!("onboard.channels.slack_step3"));
    println!();

    let token: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.slack_token_prompt")))
        .interact_text()?;

    if token.trim().is_empty() {
        println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
        return Ok(());
    }

    print!("  › {}... ", t!("onboard.channels.testing"));
    let client = reqwest::blocking::Client::new();
    match client
        .get("https://slack.com/api/auth.test")
        .bearer_auth(&token)
        .send()
    {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().unwrap_or_default();
            let ok = data
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let team = data
                .get("team")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            if ok {
                println!(
                    "\r  ✓ {}        ",
                    t!("onboard.channels.slack_workspace_connected", team = team)
                );
            } else {
                let err = data
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown error");
                println!("\r  ✗ {}", t!("onboard.channels.slack_error", error = err));
                return Ok(());
            }
        }
        _ => {
            println!("\r  ✗ {}", t!("onboard.channels.test_fail"));
            return Ok(());
        }
    }

    let app_token: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.slack_app_token_prompt")
        ))
        .allow_empty(true)
        .interact_text()?;

    let channel: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.slack_channel_prompt")))
        .allow_empty(true)
        .interact_text()?;

    print_bullet(&t!("onboard.channels.slack_allowlist_hint"));
    print_bullet(&t!("onboard.channels.slack_allowlist_format"));
    print_bullet(&t!("onboard.channels.slack_allowlist_star"));

    let allowed_users_str: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.slack_users_prompt")))
        .allow_empty(true)
        .interact_text()?;

    let allowed_users = parse_allowlist(&allowed_users_str);

    if allowed_users.is_empty() {
        println!("  ! {}", t!("onboard.channels.slack_no_users"));
    }

    config.slack = Some(SlackConfig {
        bot_token: token,
        app_token: if app_token.is_empty() {
            None
        } else {
            Some(app_token)
        },
        channel_id: if channel.is_empty() {
            None
        } else {
            Some(channel)
        },
        allowed_users,
    });

    Ok(())
}

fn setup_imessage(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.imessage_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.imessage_subtitle"))).dim()
    );

    if !cfg!(target_os = "macos") {
        println!("  ! {}", t!("onboard.channels.imessage_macos_only"));
        return Ok(());
    }

    print_bullet(&t!("onboard.channels.imessage_desc"));
    print_bullet(&t!("onboard.channels.imessage_disk_access"));
    println!();

    let contacts_str: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.imessage_contacts_prompt")
        ))
        .default("*".into())
        .interact_text()?;

    let allowed_contacts = parse_allowlist(&contacts_str);

    config.imessage = Some(IMessageConfig {
        allowed_contacts: if allowed_contacts.is_empty() {
            vec!["*".into()]
        } else {
            allowed_contacts
        },
    });
    println!(
        "  ✓ {}",
        t!(
            "onboard.channels.imessage_confirm",
            contacts = style(&contacts_str).cyan()
        )
    );

    Ok(())
}

fn setup_matrix(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.matrix_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.matrix_subtitle"))).dim()
    );
    print_bullet(&t!("onboard.channels.matrix_desc"));
    print_bullet(&t!("onboard.channels.matrix_token_hint"));
    println!();

    let homeserver: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.matrix_homeserver_prompt")
        ))
        .interact_text()?;

    if homeserver.trim().is_empty() {
        println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
        return Ok(());
    }

    let access_token: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.matrix_token_prompt")))
        .interact_text()?;

    if access_token.trim().is_empty() {
        println!(
            "  {} {}",
            style("→").dim(),
            t!("onboard.channels.matrix_token_required")
        );
        return Ok(());
    }

    let hs = homeserver.trim_end_matches('/');
    print!("  › {}... ", t!("onboard.channels.testing"));
    let client = reqwest::blocking::Client::new();
    match client
        .get(format!("{hs}/_matrix/client/v3/account/whoami"))
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
    {
        Ok(resp) if resp.status().is_success() => {
            let data: serde_json::Value = resp.json().unwrap_or_default();
            let user_id = data
                .get("user_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            println!(
                "\r  ✓ {}        ",
                t!("onboard.channels.test_success", name = user_id)
            );
        }
        _ => {
            println!("\r  ✗ {}", t!("onboard.channels.matrix_test_fail"));
            return Ok(());
        }
    }

    let room_id: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.matrix_room_prompt")))
        .interact_text()?;

    let users_str: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.matrix_users_prompt")))
        .default("*".into())
        .interact_text()?;

    let allowed_users = parse_allowlist(&users_str);

    config.matrix = Some(MatrixConfig {
        homeserver: homeserver.trim_end_matches('/').to_string(),
        access_token,
        room_id,
        allowed_users: if allowed_users.is_empty() {
            vec!["*".into()]
        } else {
            allowed_users
        },
    });

    Ok(())
}

fn setup_whatsapp(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.whatsapp_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.whatsapp_subtitle"))).dim()
    );
    print_bullet(&t!("onboard.channels.whatsapp_step1"));
    print_bullet(&t!("onboard.channels.whatsapp_step2"));
    print_bullet(&t!("onboard.channels.whatsapp_step3"));
    print_bullet(&t!("onboard.channels.whatsapp_step4"));
    println!();

    let access_token: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.whatsapp_token_prompt")
        ))
        .interact_text()?;

    if access_token.trim().is_empty() {
        println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
        return Ok(());
    }

    let phone_number_id: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.whatsapp_phone_prompt")
        ))
        .interact_text()?;

    if phone_number_id.trim().is_empty() {
        println!(
            "  {} {}",
            style("→").dim(),
            t!("onboard.channels.whatsapp_phone_required")
        );
        return Ok(());
    }

    let verify_token: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.whatsapp_verify_prompt")
        ))
        .default("asteroniris-whatsapp-verify".into())
        .interact_text()?;

    print!("  › {}... ", t!("onboard.channels.testing"));
    let client = reqwest::blocking::Client::new();
    let url = format!(
        "https://graph.facebook.com/v18.0/{}",
        phone_number_id.trim()
    );
    match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token.trim()))
        .send()
    {
        Ok(resp) if resp.status().is_success() => {
            println!(
                "\r  ✓ {}        ",
                t!("onboard.channels.whatsapp_test_success")
            );
        }
        _ => {
            println!("\r  ✗ {}", t!("onboard.channels.whatsapp_test_fail"));
            return Ok(());
        }
    }

    let users_str: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.whatsapp_numbers_prompt")
        ))
        .default("*".into())
        .interact_text()?;

    let allowed_numbers = parse_allowlist(&users_str);

    config.whatsapp = Some(WhatsAppConfig {
        access_token: access_token.trim().to_string(),
        phone_number_id: phone_number_id.trim().to_string(),
        verify_token: verify_token.trim().to_string(),
        allowed_numbers: if allowed_numbers.is_empty() {
            vec!["*".into()]
        } else {
            allowed_numbers
        },
        app_secret: None,
    });

    Ok(())
}

#[allow(clippy::too_many_lines)]
fn setup_irc(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.irc_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.irc_subtitle"))).dim()
    );
    print_bullet(&t!("onboard.channels.irc_desc"));
    print_bullet(&t!("onboard.channels.irc_sasl"));
    println!();

    let server: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_server_prompt")))
        .interact_text()?;

    if server.trim().is_empty() {
        println!("  {} {}", style("→").dim(), t!("onboard.channels.skipped"));
        return Ok(());
    }

    let port_str: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_port_prompt")))
        .default("6697".into())
        .interact_text()?;

    let port: u16 = if let Ok(p) = port_str.trim().parse() {
        p
    } else {
        println!(
            "  {} {}",
            style("→").dim(),
            t!("onboard.channels.irc_port_invalid")
        );
        6697
    };

    let nickname: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_nick_prompt")))
        .interact_text()?;

    if nickname.trim().is_empty() {
        println!(
            "  {} {}",
            style("→").dim(),
            t!("onboard.channels.irc_nick_required")
        );
        return Ok(());
    }

    let channels_str: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_channels_prompt")))
        .allow_empty(true)
        .interact_text()?;

    let channels = if channels_str.trim().is_empty() {
        vec![]
    } else {
        channels_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    print_bullet(&t!("onboard.channels.irc_allowlist_hint"));
    print_bullet(&t!("onboard.channels.irc_allowlist_star"));

    let users_str: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_users_prompt")))
        .allow_empty(true)
        .interact_text()?;

    let allowed_users = parse_allowlist(&users_str);

    if allowed_users.is_empty() {
        print_bullet(&format!("! {}", t!("onboard.channels.irc_empty_allowlist")));
    }

    println!();
    print_bullet(&t!("onboard.channels.irc_auth_header"));

    let server_password: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_server_password")))
        .allow_empty(true)
        .interact_text()?;

    let nickserv_password: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.irc_nickserv_password")
        ))
        .allow_empty(true)
        .interact_text()?;

    let sasl_password: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_sasl_password")))
        .allow_empty(true)
        .interact_text()?;

    let verify_tls: bool = Confirm::new()
        .with_prompt(format!("  {}", t!("onboard.channels.irc_verify_tls")))
        .default(true)
        .interact()?;

    println!(
        "  ✓ {}",
        t!(
            "onboard.channels.irc_confirm",
            nick = style(&nickname).cyan(),
            server = style(&server).cyan(),
            port = style(port).cyan()
        )
    );

    config.irc = Some(IrcConfig {
        server: server.trim().to_string(),
        port,
        nickname: nickname.trim().to_string(),
        username: None,
        channels,
        allowed_users: if allowed_users.is_empty() {
            vec!["*".into()]
        } else {
            allowed_users
        },
        server_password: if server_password.trim().is_empty() {
            None
        } else {
            Some(server_password.trim().to_string())
        },
        nickserv_password: if nickserv_password.trim().is_empty() {
            None
        } else {
            Some(nickserv_password.trim().to_string())
        },
        sasl_password: if sasl_password.trim().is_empty() {
            None
        } else {
            Some(sasl_password.trim().to_string())
        },
        verify_tls: Some(verify_tls),
    });

    Ok(())
}

fn setup_webhook(config: &mut ChannelsConfig) -> Result<()> {
    println!();
    println!(
        "  {} {}",
        style(t!("onboard.channels.webhook_setup")).white().bold(),
        style(format!("— {}", t!("onboard.channels.webhook_subtitle"))).dim()
    );

    let port: String = Input::new()
        .with_prompt(format!("  {}", t!("onboard.channels.webhook_port_prompt")))
        .default("8080".into())
        .interact_text()?;

    let secret: String = Input::new()
        .with_prompt(format!(
            "  {}",
            t!("onboard.channels.webhook_secret_prompt")
        ))
        .allow_empty(true)
        .interact_text()?;

    config.webhook = Some(WebhookConfig {
        port: port.parse().unwrap_or(8080),
        secret: if secret.is_empty() {
            None
        } else {
            Some(secret)
        },
    });
    println!(
        "  ✓ {}",
        t!(
            "onboard.channels.webhook_confirm",
            port = style(&port).cyan()
        )
    );

    Ok(())
}

fn parse_allowlist(input: &str) -> Vec<String> {
    if input.trim() == "*" {
        return vec!["*".into()];
    }
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
