use crate::config::{
    AutonomyConfig, BrowserConfig, ChannelsConfig, ComposioConfig, Config, HeartbeatConfig,
    MediaConfig, MemoryConfig, ObservabilityConfig, PersonaConfig, RuntimeConfig, SecretsConfig,
};
use anyhow::{Context, Result};
use dialoguer::Confirm;
use std::fs;

use crate::security::auth::AuthProfileStore;
use crate::ui::style as ui;

use super::domain::default_model_for_provider;
use super::prompts::{
    ProjectContext, setup_channels, setup_memory, setup_project_context, setup_provider,
    setup_tool_mode, setup_tunnel, setup_workspace,
};
use super::scaffold::scaffold_workspace;
use super::view::{print_step, print_summary, print_welcome_banner};

/// Run the interactive wizard. Uses TUI if stdout is a terminal, falls back to dialoguer CLI.
pub async fn run_wizard(install_daemon_flag: bool) -> Result<(Config, bool)> {
    // Detect locale before anything else
    if let Ok(lang) = std::env::var("ASTERONIRIS_LANG")
        && !lang.is_empty()
    {
        rust_i18n::set_locale(&lang);
    }

    // TUI dispatch: use full-screen TUI if stdout is a terminal
    #[cfg(feature = "tui")]
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        match super::tui::run_tui_wizard() {
            Ok(config) => {
                print_summary(&config);
                offer_install_daemon(install_daemon_flag, &config)?;
                let autostart = offer_launch_channels(&config)?;
                return Ok((config, autostart));
            }
            Err(e) => {
                tracing::warn!("TUI wizard failed, falling back to CLI: {e}");
            }
        }
    }

    run_wizard_cli(install_daemon_flag).await
}

/// CLI-based wizard using dialoguer (fallback for non-TTY or TUI failure).
async fn run_wizard_cli(install_daemon_flag: bool) -> Result<(Config, bool)> {
    print_welcome_banner();

    print_step(1, 8, &t!("onboard.step.workspace"));
    let (workspace_dir, config_path) = setup_workspace()?;

    print_step(2, 8, &t!("onboard.step.provider"));
    let (provider, api_key, model, oauth_source) = setup_provider()?;

    print_step(3, 8, &t!("onboard.step.channels"));
    let channels_config = setup_channels().await?;

    print_step(4, 8, &t!("onboard.step.tunnel"));
    let tunnel_config = setup_tunnel()?;

    print_step(5, 8, &t!("onboard.step.tool_mode"));
    let (composio_config, secrets_config) = setup_tool_mode()?;

    print_step(6, 8, &t!("onboard.step.memory"));
    let memory_config = setup_memory()?;

    print_step(7, 8, &t!("onboard.step.context"));
    let project_ctx = setup_project_context()?;

    print_step(8, 8, &t!("onboard.step.scaffold"));
    scaffold_workspace(&workspace_dir, &project_ctx)?;

    // ── Build config ──
    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key.clone())
        },
        default_provider: Some(provider.clone()),
        default_model: Some(model),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        heartbeat: HeartbeatConfig::default(),
        channels_config,
        memory: memory_config,
        media: MediaConfig::default(),
        tunnel: tunnel_config,
        gateway: crate::config::GatewayConfig::default(),
        composio: composio_config,
        secrets: secrets_config,
        browser: BrowserConfig::default(),
        persona: PersonaConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        tools: crate::config::ToolsConfig::default(),
        mcp: crate::config::McpConfig::default(),
        taste: crate::config::TasteConfig::default(),
        locale: String::from("en"),
    };

    println!(
        "  {} {}",
        ui::success("✓"),
        t!("onboard.security_confirm", level = "Supervised")
    );
    println!(
        "  {} {}",
        ui::success("✓"),
        t!(
            "onboard.memory_confirm",
            backend = &config.memory.backend,
            auto_save = if config.memory.auto_save { "on" } else { "off" }
        )
    );

    config.save()?;

    if !api_key.trim().is_empty() {
        let mut auth_store = AuthProfileStore::load_or_init_for_config(&config)?;
        let profile_id = format!(
            "{}-onboard-default",
            provider.replace([':', '/'], "-").to_ascii_lowercase()
        );
        auth_store.upsert_profile(
            crate::security::auth::AuthProfile {
                id: profile_id.clone(),
                provider: provider.clone(),
                label: Some("Created by onboarding".into()),
                api_key: Some(api_key.clone()),
                refresh_token: None,
                auth_scheme: Some(if oauth_source.is_some() {
                    "oauth".into()
                } else {
                    "api_key".into()
                }),
                oauth_source,
                disabled: false,
            },
            true,
        )?;
        auth_store.mark_profile_used(&provider, &profile_id);
        auth_store.save_for_config(&config)?;
    }

    print_summary(&config);
    offer_install_daemon(install_daemon_flag, &config)?;
    let autostart = offer_launch_channels(&config)?;

    Ok((config, autostart))
}

fn offer_install_daemon(install_daemon_flag: bool, config: &Config) -> Result<()> {
    if install_daemon_flag {
        crate::platform::service::handle_command(&crate::ServiceCommands::Install, config)?;
        println!("  {} Daemon installed as OS service", ui::success("✓"));
    } else {
        let install: bool = Confirm::new()
            .with_prompt("  › Install AsteronIris as an OS service (auto-start on boot)?")
            .default(false)
            .interact()?;

        if install {
            crate::platform::service::handle_command(&crate::ServiceCommands::Install, config)?;
            println!("  {} Daemon installed as OS service", ui::success("✓"));
        } else {
            println!(
                "  {} You can install later with: asteroniris service install",
                ui::dim("›")
            );
        }
    }

    Ok(())
}

fn offer_launch_channels(config: &Config) -> Result<bool> {
    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
        || config.channels_config.email.is_some();

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!("  › {}", t!("onboard.launch_prompt")))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!("  › {}", ui::header(t!("onboard.launching")));
            println!();
            return Ok(true);
        }
    }

    Ok(false)
}

/// Interactive repair flow: rerun channel setup only without redoing full onboarding.
pub async fn run_channels_repair_wizard() -> Result<(Config, bool)> {
    print_welcome_banner();
    println!("  {}", ui::header(t!("onboard.repair.title")));
    println!();

    let mut config = Config::load_or_init()?;

    print_step(1, 1, &t!("onboard.step.channels"));
    config.channels_config = setup_channels().await?;
    config.save()?;

    println!();
    println!(
        "  {} {}",
        ui::success("✓"),
        t!("onboard.repair.saved", path = config.config_path.display())
    );

    let autostart = offer_launch_channels(&config)?;

    Ok((config, autostart))
}

// ── Quick setup (zero prompts) ───────────────────────────────────

/// Non-interactive setup: generates a sensible default config instantly.
#[allow(clippy::too_many_lines)]
pub fn run_quick_setup(
    api_key: Option<&str>,
    provider: Option<&str>,
    memory_backend: Option<&str>,
    install_daemon_flag: bool,
) -> Result<(Config, bool)> {
    print_welcome_banner();
    println!("  {}", ui::header(t!("onboard.quick.title")));
    println!();

    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;
    let asteroniris_dir = home.join(".asteroniris");
    let workspace_dir = asteroniris_dir.join("workspace");
    let config_path = asteroniris_dir.join("config.toml");

    fs::create_dir_all(&workspace_dir).context("Failed to create workspace directory")?;

    let provider_name = provider.unwrap_or("openrouter").to_string();
    let model = default_model_for_provider(&provider_name);
    let memory_backend_name = memory_backend.unwrap_or("sqlite").to_string();

    let memory_config = MemoryConfig {
        backend: memory_backend_name.clone(),
        auto_save: memory_backend_name != "none",
        hygiene_enabled: memory_backend_name == "sqlite",
        archive_after_days: if memory_backend_name == "sqlite" {
            7
        } else {
            0
        },
        purge_after_days: if memory_backend_name == "sqlite" {
            30
        } else {
            0
        },
        conversation_retention_days: 30,
        layer_retention_working_days: None,
        layer_retention_episodic_days: None,
        layer_retention_semantic_days: None,
        layer_retention_procedural_days: None,
        layer_retention_identity_days: None,
        ledger_retention_days: None,
        embedding_provider: "none".to_string(),
        embedding_model: "text-embedding-3-small".to_string(),
        embedding_dimensions: 1536,
        vector_weight: 0.7,
        keyword_weight: 0.3,
        embedding_cache_size: if memory_backend_name == "sqlite" {
            10000
        } else {
            0
        },
        chunk_max_tokens: 512,
    };

    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: api_key.map(String::from),
        default_provider: Some(provider_name.clone()),
        default_model: Some(model.clone()),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        heartbeat: HeartbeatConfig::default(),
        channels_config: ChannelsConfig::default(),
        memory: memory_config,
        media: MediaConfig::default(),
        tunnel: crate::config::TunnelConfig::default(),
        gateway: crate::config::GatewayConfig::default(),
        composio: ComposioConfig::default(),
        secrets: SecretsConfig::default(),
        browser: BrowserConfig::default(),
        persona: PersonaConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        tools: crate::config::ToolsConfig::default(),
        mcp: crate::config::McpConfig::default(),
        taste: crate::config::TasteConfig::default(),
        locale: String::from("en"),
    };

    config.save()?;

    // Scaffold minimal workspace files
    let default_ctx = ProjectContext {
        user_name: std::env::var("USER").unwrap_or_else(|_| "User".into()),
        timezone: "UTC".into(),
        agent_name: "AsteronIris".into(),
        communication_style:
            "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing."
                .into(),
    };
    scaffold_workspace(&workspace_dir, &default_ctx)?;

    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.workspace"),
        ui::value(workspace_dir.display())
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.provider"),
        ui::value(&provider_name)
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.model"),
        ui::value(&model)
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.api_key"),
        if api_key.is_some() {
            ui::value(t!("onboard.quick.api_key_set"))
        } else {
            ui::yellow(t!("onboard.quick.api_key_not_set"))
        }
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.security"),
        ui::value(t!("onboard.quick.security_value"))
    );
    println!(
        "  {} {} {} (auto-save: {})",
        ui::success("✓"),
        t!("onboard.quick.memory"),
        ui::value(&memory_backend_name),
        if memory_backend_name == "none" {
            "off"
        } else {
            "on"
        }
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.secrets"),
        ui::value(t!("onboard.quick.secrets_value"))
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.gateway"),
        ui::value(t!("onboard.quick.gateway_value"))
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.tunnel"),
        ui::dim(t!("onboard.quick.tunnel_value"))
    );
    println!(
        "  {} {} {}",
        ui::success("✓"),
        t!("onboard.quick.composio"),
        ui::dim(t!("onboard.quick.composio_value"))
    );
    println!();
    println!(
        "  {} {}",
        ui::header(t!("onboard.quick.config_saved")),
        ui::value(config_path.display())
    );
    println!();
    println!("  {}", ui::header(t!("onboard.summary.next_steps")));
    if api_key.is_none() {
        println!("    1. Set your API key:  export OPENROUTER_API_KEY=\"sk-...\"");
        println!("    2. Or edit:           ~/.asteroniris/config.toml");
        println!("    3. Chat:              asteroniris agent -m \"Hello!\"");
        println!("    4. Gateway:           asteroniris gateway");
    } else {
        println!("    1. Chat:     asteroniris agent -m \"Hello!\"");
        println!("    2. Gateway:  asteroniris gateway");
        println!("    3. Status:   asteroniris status");
    }
    println!();

    offer_install_daemon(install_daemon_flag, &config)?;

    Ok((config, false))
}
