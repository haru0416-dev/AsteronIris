use anyhow::{Result, bail};
use tracing::info;

use crate::Config;
use crate::app::status::render_status;
use crate::cli::commands::{ChannelCommands, Cli, Commands};

pub async fn dispatch(cli: Cli, config: Config) -> Result<()> {
    // Onboard runs quick setup by default, or the interactive wizard with --interactive
    if let Commands::Onboard {
        interactive,
        channels_only,
        api_key,
        provider,
        memory,
    } = &cli.command
    {
        if *interactive && *channels_only {
            bail!("Use either --interactive or --channels-only, not both");
        }
        if *channels_only && (api_key.is_some() || provider.is_some() || memory.is_some()) {
            bail!("--channels-only does not accept --api-key, --provider, or --memory");
        }

        let config = if *channels_only {
            crate::onboard::run_channels_repair_wizard()?
        } else if *interactive {
            crate::onboard::run_wizard()?
        } else {
            crate::onboard::run_quick_setup(
                api_key.as_deref(),
                provider.as_deref(),
                memory.as_deref(),
            )?
        };
        // Auto-start channels if user said yes during wizard
        if std::env::var("ASTERONIRIS_AUTOSTART_CHANNELS").as_deref() == Ok("1") {
            crate::channels::start_channels(config).await?;
        }
        return Ok(());
    }

    match cli.command {
        Commands::Onboard { .. } => unreachable!(),

        Commands::Agent {
            message,
            provider,
            model,
            temperature,
        } => crate::agent::run(config, message, provider, model, temperature).await,

        Commands::Gateway { port, host } => {
            if port == 0 {
                info!("🚀 Starting AsteronIris Gateway on {host} (random port)");
            } else {
                info!("🚀 Starting AsteronIris Gateway on {host}:{port}");
            }
            crate::gateway::run_gateway(&host, port, config).await
        }

        Commands::Daemon { port, host } => {
            if port == 0 {
                info!("🧠 Starting AsteronIris Daemon on {host} (random port)");
            } else {
                info!("🧠 Starting AsteronIris Daemon on {host}:{port}");
            }
            crate::daemon::run(config, host, port).await
        }

        Commands::Status => {
            println!("{}", render_status(&config));
            Ok(())
        }

        Commands::Cron { cron_command } => crate::cron::handle_command(cron_command, &config),

        Commands::Service { service_command } => {
            crate::service::handle_command(&service_command, &config)
        }

        Commands::Doctor => crate::doctor::run(&config),

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => crate::channels::start_channels(config).await,
            ChannelCommands::Doctor => crate::channels::doctor_channels(config).await,
            other => crate::channels::handle_command(other, &config),
        },

        Commands::Integrations {
            integration_command,
        } => crate::integrations::handle_command(integration_command, &config),

        Commands::Auth { auth_command } => crate::auth::handle_command(auth_command, &config),

        Commands::Skills { skill_command } => {
            crate::skills::handle_command(skill_command, &config.workspace_dir)
        }
    }
}
