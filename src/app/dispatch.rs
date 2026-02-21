use anyhow::{Result, bail};
use asteroniris::ChannelCommands;
use std::sync::Arc;
use tracing::info;

use crate::app::status::render_status;
use crate::cli::commands::{Cli, Commands};
use asteroniris::Config;

#[allow(clippy::too_many_lines)]
pub async fn dispatch(cli: Cli, config: Arc<Config>) -> Result<()> {
    // Onboard runs quick setup by default, or the interactive wizard with --interactive
    if let Commands::Onboard {
        interactive,
        channels_only,
        api_key,
        provider,
        memory,
        install_daemon,
    } = &cli.command
    {
        if *interactive && *channels_only {
            bail!("Use either --interactive or --channels-only, not both");
        }
        if *channels_only && (api_key.is_some() || provider.is_some() || memory.is_some()) {
            bail!("--channels-only does not accept --api-key, --provider, or --memory");
        }

        let (config, autostart) = if *channels_only {
            asteroniris::onboard::run_channels_repair_wizard()?
        } else if *interactive {
            asteroniris::onboard::run_wizard(*install_daemon)?
        } else {
            asteroniris::onboard::run_quick_setup(
                api_key.as_deref(),
                provider.as_deref(),
                memory.as_deref(),
                *install_daemon,
            )?
        };
        // Auto-start channels if user said yes during wizard
        if autostart {
            asteroniris::transport::channels::start_channels(Arc::new(config)).await?;
        }
        return Ok(());
    }

    // ── Auto-onboard for commands that need a configured provider ──
    let config = if matches!(
        &cli.command,
        Commands::Agent { .. } | Commands::Gateway { .. } | Commands::Daemon { .. }
    ) && config.needs_onboarding()
    {
        use asteroniris::ui::style as ui;
        println!();
        println!(
            "  {} {}",
            ui::accent("◆"),
            ui::header("Welcome to AsteronIris!")
        );
        println!(
            "  {}",
            ui::dim("No configuration found. Let's set things up first.")
        );
        println!();

        let (new_config, _autostart) = asteroniris::onboard::run_wizard(false)?;
        Arc::new(new_config)
    } else {
        config
    };

    match cli.command {
        Commands::Onboard { .. } => unreachable!(),

        Commands::Agent {
            message,
            provider,
            model,
            temperature,
        } => {
            asteroniris::core::agent::run(
                Arc::clone(&config),
                message,
                provider,
                model,
                temperature,
            )
            .await
        }

        Commands::Gateway { port, host } => {
            if port == 0 {
                info!("🚀 Starting AsteronIris Gateway on {host} (random port)");
            } else {
                info!("🚀 Starting AsteronIris Gateway on {host}:{port}");
            }
            asteroniris::transport::gateway::run_gateway(&host, port, Arc::clone(&config)).await
        }

        Commands::Daemon { port, host } => {
            if port == 0 {
                info!("🧠 Starting AsteronIris Daemon on {host} (random port)");
            } else {
                info!("🧠 Starting AsteronIris Daemon on {host}:{port}");
            }
            asteroniris::platform::daemon::run(Arc::clone(&config), host, port).await
        }

        Commands::Status => {
            println!("{}", render_status(&config));
            Ok(())
        }

        Commands::Cron { cron_command } => {
            asteroniris::platform::cron::handle_command(cron_command, &config)
        }

        Commands::Service { service_command } => {
            asteroniris::platform::service::handle_command(&service_command, &config)
        }

        Commands::Doctor => asteroniris::runtime::diagnostics::doctor::run(&config),

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => {
                asteroniris::transport::channels::start_channels(Arc::clone(&config)).await
            }
            ChannelCommands::Doctor => {
                asteroniris::transport::channels::doctor_channels(Arc::clone(&config)).await
            }
            other => asteroniris::transport::channels::handle_command(other, &config),
        },

        Commands::Integrations {
            integration_command,
        } => asteroniris::plugins::integrations::handle_command(integration_command, &config),

        Commands::Auth { auth_command } => {
            asteroniris::security::auth::handle_command(auth_command, &config)
        }

        Commands::Skills { skill_command } => {
            asteroniris::plugins::skills::handle_command(skill_command, &config.workspace_dir)
        }
    }
}
