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
    print_bullet("Channels let you talk to AsteronIris from anywhere.");
    print_bullet("CLI is always available. Connect more channels now.");
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
        let options = vec![
            format!(
                "Telegram   {}",
                if config.telegram.is_some() {
                    "✅ connected"
                } else {
                    "— connect your bot"
                }
            ),
            format!(
                "Discord    {}",
                if config.discord.is_some() {
                    "✅ connected"
                } else {
                    "— connect your bot"
                }
            ),
            format!(
                "Slack      {}",
                if config.slack.is_some() {
                    "✅ connected"
                } else {
                    "— connect your bot"
                }
            ),
            format!(
                "iMessage   {}",
                if config.imessage.is_some() {
                    "✅ configured"
                } else {
                    "— macOS only"
                }
            ),
            format!(
                "Matrix     {}",
                if config.matrix.is_some() {
                    "✅ connected"
                } else {
                    "— self-hosted chat"
                }
            ),
            format!(
                "WhatsApp   {}",
                if config.whatsapp.is_some() {
                    "✅ connected"
                } else {
                    "— Business Cloud API"
                }
            ),
            format!(
                "IRC        {}",
                if config.irc.is_some() {
                    "✅ configured"
                } else {
                    "— IRC over TLS"
                }
            ),
            format!(
                "Webhook    {}",
                if config.webhook.is_some() {
                    "✅ configured"
                } else {
                    "— HTTP endpoint"
                }
            ),
            "Done — finish setup".to_string(),
        ];

        let choice = Select::new()
            .with_prompt("  Connect a channel (or Done to continue)")
            .items(&options)
            .default(8)
            .interact()?;

        match choice {
            0 => {
                // ── Telegram ──
                println!();
                println!(
                    "  {} {}",
                    style("Telegram Setup").white().bold(),
                    style("— talk to AsteronIris from Telegram").dim()
                );
                print_bullet("1. Open Telegram and message @BotFather");
                print_bullet("2. Send /newbot and follow the prompts");
                print_bullet("3. Copy the bot token and paste it below");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot token (from @BotFather)")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
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
                            "\r  {} Connected as @{bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token and try again",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                print_bullet(
                    "Allowlist your own Telegram identity first (recommended for secure + fast setup).",
                );
                print_bullet(
                    "Use your @username without '@' (example: argenis), or your numeric Telegram user ID.",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Telegram identities (comma-separated: username without '@' and/or numeric user ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Telegram inbound messages will be denied until you add your username/user ID or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.telegram = Some(TelegramConfig {
                    bot_token: token,
                    allowed_users,
                });
            }
            1 => {
                // ── Discord ──
                println!();
                println!(
                    "  {} {}",
                    style("Discord Setup").white().bold(),
                    style("— talk to AsteronIris from Discord").dim()
                );
                print_bullet("1. Go to https://discord.com/developers/applications");
                print_bullet("2. Create a New Application → Bot → Copy token");
                print_bullet("3. Enable MESSAGE CONTENT intent under Bot settings");
                print_bullet("4. Invite bot to your server with messages permission");
                println!();

                let token: String = Input::new().with_prompt("  Bot token").interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
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
                            "\r  {} Connected as {bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token and try again",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let guild: String = Input::new()
                    .with_prompt("  Server (guild) ID (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("Allowlist your own Discord user ID first (recommended).");
                print_bullet(
                    "Get it in Discord: Settings -> Advanced -> Developer Mode (ON), then right-click your profile -> Copy User ID.",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Discord user IDs (comma-separated, recommended: your own ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Discord inbound messages will be denied until you add IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.discord = Some(DiscordConfig {
                    bot_token: token,
                    guild_id: if guild.is_empty() { None } else { Some(guild) },
                    allowed_users,
                });
            }
            2 => {
                // ── Slack ──
                println!();
                println!(
                    "  {} {}",
                    style("Slack Setup").white().bold(),
                    style("— talk to AsteronIris from Slack").dim()
                );
                print_bullet("1. Go to https://api.slack.com/apps → Create New App");
                print_bullet("2. Add Bot Token Scopes: chat:write, channels:history");
                print_bullet("3. Install to workspace and copy the Bot Token");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot token (xoxb-...)")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
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
                                "\r  {} Connected to workspace: {team}        ",
                                style("✅").green().bold()
                            );
                        } else {
                            let err = data
                                .get("error")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown error");
                            println!("\r  {} Slack error: {err}", style("❌").red().bold());
                            continue;
                        }
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let app_token: String = Input::new()
                    .with_prompt("  App token (xapp-..., optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                let channel: String = Input::new()
                    .with_prompt("  Default channel ID (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("Allowlist your own Slack member ID first (recommended).");
                print_bullet(
                    "Member IDs usually start with 'U' (open your Slack profile -> More -> Copy member ID).",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Slack user IDs (comma-separated, recommended: your own member ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Slack inbound messages will be denied until you add IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
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
            }
            3 => {
                // ── iMessage ──
                println!();
                println!(
                    "  {} {}",
                    style("iMessage Setup").white().bold(),
                    style("— macOS only, reads from Messages.app").dim()
                );

                if !cfg!(target_os = "macos") {
                    println!(
                        "  {} iMessage is only available on macOS.",
                        style("⚠").yellow().bold()
                    );
                    continue;
                }

                print_bullet(
                    "AsteronIris reads your iMessage database and replies via AppleScript.",
                );
                print_bullet(
                    "You need to grant Full Disk Access to your terminal in System Settings.",
                );
                println!();

                let contacts_str: String = Input::new()
                    .with_prompt("  Allowed contacts (comma-separated phone/email, or * for all)")
                    .default("*".into())
                    .interact_text()?;

                let allowed_contacts = if contacts_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    contacts_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect()
                };

                config.imessage = Some(IMessageConfig { allowed_contacts });
                println!(
                    "  {} iMessage configured (contacts: {})",
                    style("✅").green().bold(),
                    style(&contacts_str).cyan()
                );
            }
            4 => {
                // ── Matrix ──
                println!();
                println!(
                    "  {} {}",
                    style("Matrix Setup").white().bold(),
                    style("— self-hosted, federated chat").dim()
                );
                print_bullet("You need a Matrix account and an access token.");
                print_bullet("Get a token via Element → Settings → Help & About → Access Token.");
                println!();

                let homeserver: String = Input::new()
                    .with_prompt("  Homeserver URL (e.g. https://matrix.org)")
                    .interact_text()?;

                if homeserver.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let access_token: String =
                    Input::new().with_prompt("  Access token").interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} Skipped — token required", style("→").dim());
                    continue;
                }

                // Test connection
                let hs = homeserver.trim_end_matches('/');
                print!("  {} Testing connection... ", style("⏳").dim());
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
                            "\r  {} Connected as {user_id}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check homeserver URL and token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let room_id: String = Input::new()
                    .with_prompt("  Room ID (e.g. !abc123:matrix.org)")
                    .interact_text()?;

                let users_str: String = Input::new()
                    .with_prompt("  Allowed users (comma-separated @user:server, or * for all)")
                    .default("*".into())
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.matrix = Some(MatrixConfig {
                    homeserver: homeserver.trim_end_matches('/').to_string(),
                    access_token,
                    room_id,
                    allowed_users,
                });
            }
            5 => {
                // ── WhatsApp ──
                println!();
                println!(
                    "  {} {}",
                    style("WhatsApp Setup").white().bold(),
                    style("— Business Cloud API").dim()
                );
                print_bullet("1. Go to developers.facebook.com and create a WhatsApp app");
                print_bullet("2. Add the WhatsApp product and get your phone number ID");
                print_bullet("3. Generate a temporary access token (System User)");
                print_bullet("4. Configure webhook URL to: https://your-domain/whatsapp");
                println!();

                let access_token: String = Input::new()
                    .with_prompt("  Access token (from Meta Developers)")
                    .interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let phone_number_id: String = Input::new()
                    .with_prompt("  Phone number ID (from WhatsApp app settings)")
                    .interact_text()?;

                if phone_number_id.trim().is_empty() {
                    println!("  {} Skipped — phone number ID required", style("→").dim());
                    continue;
                }

                let verify_token: String = Input::new()
                    .with_prompt("  Webhook verify token (create your own)")
                    .default("asteroniris-whatsapp-verify".into())
                    .interact_text()?;

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
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
                            "\r  {} Connected to WhatsApp API        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check access token and phone number ID",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed phone numbers (comma-separated +1234567890, or * for all)",
                    )
                    .default("*".into())
                    .interact_text()?;

                let allowed_numbers = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.whatsapp = Some(WhatsAppConfig {
                    access_token: access_token.trim().to_string(),
                    phone_number_id: phone_number_id.trim().to_string(),
                    verify_token: verify_token.trim().to_string(),
                    allowed_numbers,
                    app_secret: None, // Can be set via ASTERONIRIS_WHATSAPP_APP_SECRET env var
                });
            }
            6 => {
                // ── IRC ──
                println!();
                println!(
                    "  {} {}",
                    style("IRC Setup").white().bold(),
                    style("— IRC over TLS").dim()
                );
                print_bullet("IRC connects over TLS to any IRC server");
                print_bullet("Supports SASL PLAIN and NickServ authentication");
                println!();

                let server: String = Input::new()
                    .with_prompt("  IRC server (hostname)")
                    .interact_text()?;

                if server.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let port_str: String = Input::new()
                    .with_prompt("  Port")
                    .default("6697".into())
                    .interact_text()?;

                let port: u16 = if let Ok(p) = port_str.trim().parse() {
                    p
                } else {
                    println!("  {} Invalid port, using 6697", style("→").dim());
                    6697
                };

                let nickname: String =
                    Input::new().with_prompt("  Bot nickname").interact_text()?;

                if nickname.trim().is_empty() {
                    println!("  {} Skipped — nickname required", style("→").dim());
                    continue;
                }

                let channels_str: String = Input::new()
                    .with_prompt("  Channels to join (comma-separated: #channel1,#channel2)")
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

                print_bullet(
                    "Allowlist nicknames that can interact with the bot (case-insensitive).",
                );
                print_bullet("Use '*' to allow anyone (not recommended for production).");

                let users_str: String = Input::new()
                    .with_prompt("  Allowed nicknames (comma-separated, or * for all)")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    print_bullet(
                        "⚠️  Empty allowlist — only you can interact. Add nicknames above.",
                    );
                }

                println!();
                print_bullet("Optional authentication (press Enter to skip each):");

                let server_password: String = Input::new()
                    .with_prompt("  Server password (for bouncers like ZNC, leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let nickserv_password: String = Input::new()
                    .with_prompt("  NickServ password (leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let sasl_password: String = Input::new()
                    .with_prompt("  SASL PLAIN password (leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let verify_tls: bool = Confirm::new()
                    .with_prompt("  Verify TLS certificate?")
                    .default(true)
                    .interact()?;

                println!(
                    "  {} IRC configured as {}@{}:{}",
                    style("✅").green().bold(),
                    style(&nickname).cyan(),
                    style(&server).cyan(),
                    style(port).cyan()
                );

                config.irc = Some(IrcConfig {
                    server: server.trim().to_string(),
                    port,
                    nickname: nickname.trim().to_string(),
                    username: None,
                    channels,
                    allowed_users,
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
            }
            7 => {
                // ── Webhook ──
                println!();
                println!(
                    "  {} {}",
                    style("Webhook Setup").white().bold(),
                    style("— HTTP endpoint for custom integrations").dim()
                );

                let port: String = Input::new()
                    .with_prompt("  Port")
                    .default("8080".into())
                    .interact_text()?;

                let secret: String = Input::new()
                    .with_prompt("  Secret (optional, Enter to skip)")
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
                    "  {} Webhook on port {}",
                    style("✅").green().bold(),
                    style(&port).cyan()
                );
            }
            _ => break, // Done
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
        "  {} Channels: {}",
        style("✓").green().bold(),
        style(active.join(", ")).green()
    );

    Ok(config)
}
