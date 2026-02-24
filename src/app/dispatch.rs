use crate::cli::commands::{
    ChannelCommands, Cli, Commands, CronCommands, IntegrationCommands, ServiceCommands,
    SkillCommands,
};
use anyhow::{Result, bail};
use std::sync::Arc;
use tracing::info;

use crate::Config;
use crate::app::status::render_status;

/// Run the AI agent loop via the v2 tool-loop API.
///
/// 1. Creates an LLM provider via the resilient factory with OAuth recovery.
/// 2. Creates memory via `memory::factory::create_memory`.
/// 3. Builds the tool registry from `tools::all_tools(memory)`.
/// 4. Runs a `ToolLoop` and prints the result.
async fn run_agent(
    config: Arc<Config>,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");

    let model = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");

    let user_message = message.unwrap_or_else(|| "Hello! How can you help me today?".to_string());

    // 1. Create resilient LLM provider
    let provider = crate::llm::factory::create_resilient_provider_with_oauth_recovery(
        &config,
        provider_name,
        &config.reliability,
        |name| crate::llm::factory::resolve_api_key(name, config.api_key.as_deref()),
    )?;

    // 2. Create memory
    let memory = crate::memory::factory::create_memory(
        &config.memory,
        &config.workspace_dir,
        config.api_key.as_deref(),
    )
    .await?;

    // 3. Build tool registry
    let tools = crate::tools::all_tools(Arc::from(memory));
    let mut registry = crate::tools::ToolRegistry::default();
    for tool in tools {
        registry.register(tool);
    }
    let registry = Arc::new(registry);

    // 4. Create and run the tool loop
    let tool_loop = crate::agent::tool_loop::ToolLoop::new(Arc::clone(&registry), 10);
    let security = Arc::new(crate::security::SecurityPolicy::default());
    let ctx = crate::tools::ExecutionContext::from_security(security);

    let result = tool_loop
        .run(crate::agent::tool_loop::ToolLoopRunParams {
            provider: provider.as_ref(),
            system_prompt: "You are AsteronIris, a helpful AI assistant.",
            user_message: &user_message,
            image_content: &[],
            model,
            temperature,
            ctx: &ctx,
            stream_sink: None,
            conversation_history: &[],
            hooks: &[],
        })
        .await?;

    println!("{}", result.final_text);

    if let Some(tokens) = result.tokens_used {
        info!(
            tokens_used = tokens,
            iterations = result.iterations,
            "agent loop complete"
        );
    }

    Ok(())
}

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
            crate::onboard::run_channels_repair_wizard().await?
        } else if *interactive {
            crate::onboard::run_wizard(*install_daemon).await?
        } else {
            crate::onboard::run_quick_setup(
                api_key.as_deref(),
                provider.as_deref(),
                memory.as_deref(),
                *install_daemon,
            )?
        };
        // Auto-start channels if user said yes during wizard
        if autostart {
            crate::transport::channels::start_channels(Arc::new(config)).await?;
        }
        return Ok(());
    }

    // ── Auto-onboard for commands that need a configured provider ──
    let config = if matches!(
        &cli.command,
        Commands::Agent { .. } | Commands::Gateway { .. } | Commands::Daemon { .. }
    ) && config.needs_onboarding()
    {
        use crate::ui::style as ui;
        println!();
        println!(
            "  {} {}",
            ui::accent("*"),
            ui::header("Welcome to AsteronIris!")
        );
        println!(
            "  {}",
            ui::dim("No configuration found. Let's set things up first.")
        );
        println!();

        let (new_config, _autostart) = crate::onboard::run_wizard(false).await?;
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
        } => run_agent(Arc::clone(&config), message, provider, model, temperature).await,

        Commands::Gateway { port, host } => {
            let port = port.unwrap_or(config.gateway.port);
            let host = host.unwrap_or_else(|| config.gateway.host.clone());
            if port == 0 {
                info!("Starting AsteronIris Gateway on {host} (random port)");
            } else {
                info!("Starting AsteronIris Gateway on {host}:{port}");
            }
            crate::transport::gateway::run_gateway(&host, port, Arc::clone(&config)).await
        }

        Commands::Daemon { port, host } => {
            let port = port.unwrap_or(config.gateway.port);
            let host = host.unwrap_or_else(|| config.gateway.host.clone());
            if port == 0 {
                info!("Starting AsteronIris Daemon on {host} (random port)");
            } else {
                info!("Starting AsteronIris Daemon on {host}:{port}");
            }
            crate::platform::daemon::run(Arc::clone(&config), host, port).await
        }

        Commands::Status => {
            println!("{}", render_status(&config));
            Ok(())
        }

        Commands::Eval {
            seed: _seed,
            evidence_slug: _evidence_slug,
        } => {
            bail!("eval command is not yet available in v2")
        }

        Commands::Evolve { apply } => crate::runtime::evolution::run_cycle(&config, apply),

        Commands::Model { set, provider } => {
            let mut updated = config.as_ref().clone();
            updated.default_model = Some(set.clone());
            if let Some(provider_name) = provider.as_deref() {
                let trimmed = provider_name.trim();
                if trimmed.is_empty() {
                    bail!("--provider cannot be empty");
                }
                updated.default_provider = Some(trimmed.to_string());
            }
            updated.save()?;

            println!("Updated model defaults");
            println!(
                "Provider: {}",
                updated.default_provider.as_deref().unwrap_or("(unset)")
            );
            println!(
                "Model: {}",
                updated.default_model.as_deref().unwrap_or("(unset)")
            );
            println!("Config: {}", updated.config_path.display());
            Ok(())
        }

        Commands::Cron { cron_command } => {
            let cmd = match cron_command {
                CronCommands::List => crate::platform::cron::CronCommand::List,
                CronCommands::Add {
                    expression,
                    command,
                } => crate::platform::cron::CronCommand::Add {
                    expression,
                    command,
                },
                CronCommands::Remove { id } => crate::platform::cron::CronCommand::Remove { id },
            };
            crate::platform::cron::handle_command(cmd, &config).await
        }

        Commands::Service { service_command } => {
            let cmd = match service_command {
                ServiceCommands::Install => crate::platform::service::ServiceCommand::Install,
                ServiceCommands::Start => crate::platform::service::ServiceCommand::Start,
                ServiceCommands::Stop => crate::platform::service::ServiceCommand::Stop,
                ServiceCommands::Status => crate::platform::service::ServiceCommand::Status,
                ServiceCommands::Uninstall => crate::platform::service::ServiceCommand::Uninstall,
            };
            crate::platform::service::handle_command(&cmd, &config)
        }

        Commands::Doctor => crate::runtime::diagnostics::doctor::run(&config).await,

        Commands::Channel { channel_command } => match channel_command {
            ChannelCommands::Start => {
                crate::transport::channels::start_channels(Arc::clone(&config)).await
            }
            ChannelCommands::Doctor => {
                crate::transport::channels::doctor_channels(Arc::clone(&config)).await
            }
            ChannelCommands::List => {
                // TODO: implement channel list display
                println!("Configured channels:");
                if config.channels_config.telegram.is_some() {
                    println!("  - Telegram");
                }
                if config.channels_config.discord.is_some() {
                    println!("  - Discord");
                }
                if config.channels_config.slack.is_some() {
                    println!("  - Slack");
                }
                if config.channels_config.matrix.is_some() {
                    println!("  - Matrix");
                }
                if config.channels_config.email.is_some() {
                    println!("  - Email");
                }
                if config.channels_config.imessage.is_some() {
                    println!("  - iMessage");
                }
                Ok(())
            }
            ChannelCommands::Add {
                channel_type,
                config: _cfg,
            } => {
                bail!(
                    "Channel add not yet implemented for '{channel_type}' — edit config.toml directly"
                )
            }
            ChannelCommands::Remove { name } => {
                bail!("Channel remove not yet implemented for '{name}' — edit config.toml directly")
            }
        },

        Commands::Integrations {
            integration_command,
        } => match integration_command {
            IntegrationCommands::Info { name } => {
                crate::plugins::integrations::show_integration_info(&config, &name)
            }
        },

        Commands::Auth { auth_command: _ } => {
            // TODO: security::auth CLI not yet ported to v2
            bail!(
                "auth command not yet available in v2 -- run `asteroniris onboard` to configure API keys"
            )
        }

        Commands::Skills { skill_command } => match skill_command {
            SkillCommands::List => {
                let skills = crate::plugins::skills::load_skills(&config.workspace_dir);
                if skills.is_empty() {
                    println!("No skills installed.");
                } else {
                    println!("Installed skills:");
                    for skill in &skills {
                        println!("  - {}: {}", skill.name, skill.description);
                    }
                }
                Ok(())
            }
            SkillCommands::Install { source } => {
                bail!(
                    "Skill install not yet implemented for '{source}' — copy skill files to workspace/skills/"
                )
            }
            SkillCommands::Remove { name } => {
                bail!(
                    "Skill remove not yet implemented for '{name}' — delete from workspace/skills/"
                )
            }
        },
    }
}
