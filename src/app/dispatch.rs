use anyhow::{Result, bail};
use asteroniris::ChannelCommands;
use asteroniris::cli::commands::{Cli, Commands};
use std::sync::Arc;
use tracing::info;

use crate::app::status::render_status;
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

        Commands::Eval {
            seed,
            evidence_slug,
        } => {
            let suites = asteroniris::core::eval::default_baseline_suites();
            let harness = asteroniris::core::eval::EvalHarness::new(seed);
            let report = harness.run(&suites);

            if let Some(slug) = evidence_slug.as_deref() {
                let files = asteroniris::core::eval::write_evidence_files(
                    &config.workspace_dir,
                    &report,
                    slug,
                    None,
                )?;
                println!("wrote evidence files:");
                for path in files {
                    println!("- {}", path.display());
                }
            }

            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }

        Commands::Evolve { apply } => asteroniris::runtime::evolution::run_cycle(&config, apply),

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

            println!("✅ Updated model defaults");
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

#[cfg(test)]
mod tests {
    use super::dispatch;
    use asteroniris::Config;
    use asteroniris::cli::commands::{Cli, Commands};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn dispatch_eval_with_evidence_writes_baseline_files() {
        let tmp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();

        let cli = Cli {
            command: Commands::Eval {
                seed: 123,
                evidence_slug: Some("dispatch-eval".to_string()),
            },
        };

        dispatch(cli, Arc::new(config))
            .await
            .expect("eval dispatch should succeed");

        let evidence_dir = tmp.path().join(".sisyphus").join("evidence");
        assert!(
            evidence_dir.join("task-13-dispatch-eval.txt").exists(),
            "text evidence should be written"
        );
        assert!(
            evidence_dir
                .join("task-13-dispatch-eval-baseline-report.csv")
                .exists(),
            "csv evidence should be written"
        );
        assert!(
            evidence_dir
                .join("task-13-dispatch-eval-baseline-report.json")
                .exists(),
            "json evidence should be written"
        );
    }

    #[tokio::test]
    async fn dispatch_eval_with_unsafe_slug_writes_sanitized_paths() {
        let tmp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();

        let cli = Cli {
            command: Commands::Eval {
                seed: 456,
                evidence_slug: Some(" ../A/B C?* ".to_string()),
            },
        };

        dispatch(cli, Arc::new(config))
            .await
            .expect("eval dispatch should succeed with unsafe slug");

        let evidence_dir = tmp.path().join(".sisyphus").join("evidence");
        assert!(
            evidence_dir.join("task-13-a-b-c.txt").exists(),
            "sanitized text evidence should be written"
        );
        assert!(
            evidence_dir
                .join("task-13-a-b-c-baseline-report.csv")
                .exists(),
            "sanitized csv evidence should be written"
        );
        assert!(
            evidence_dir
                .join("task-13-a-b-c-baseline-report.json")
                .exists(),
            "sanitized json evidence should be written"
        );
    }

    #[tokio::test]
    async fn dispatch_eval_with_blank_slug_falls_back_to_default_slug() {
        let tmp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();

        let cli = Cli {
            command: Commands::Eval {
                seed: 789,
                evidence_slug: Some("   ".to_string()),
            },
        };

        dispatch(cli, Arc::new(config))
            .await
            .expect("eval dispatch should succeed with blank slug");

        let evidence_dir = tmp.path().join(".sisyphus").join("evidence");
        assert!(
            evidence_dir.join("task-13-eval.txt").exists(),
            "default slug text evidence should be written"
        );
        assert!(
            evidence_dir
                .join("task-13-eval-baseline-report.csv")
                .exists(),
            "default slug csv evidence should be written"
        );
        assert!(
            evidence_dir
                .join("task-13-eval-baseline-report.json")
                .exists(),
            "default slug json evidence should be written"
        );
    }
}
