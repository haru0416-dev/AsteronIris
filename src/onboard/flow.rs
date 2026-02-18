use crate::config::{
    AutonomyConfig, BrowserConfig, ChannelsConfig, ComposioConfig, Config, HeartbeatConfig,
    ObservabilityConfig, PersonaConfig, RuntimeConfig, SecretsConfig,
};
use anyhow::{Context, Result};
use console::style;
use dialoguer::Confirm;

use super::domain::default_model_for_provider;
use super::prompts::{
    setup_channels, setup_memory, setup_project_context, setup_provider, setup_tool_mode,
    setup_tunnel, setup_workspace, ProjectContext,
};
use super::scaffold::scaffold_workspace;
use super::view::{print_step, print_summary, print_welcome_banner};

pub fn run_wizard() -> Result<Config> {
    print_welcome_banner();

    print_step(1, 8, "Workspace Setup");
    let (workspace_dir, config_path) = setup_workspace()?;

    print_step(2, 8, "AI Provider & API Key");
    let (provider, api_key, model) = setup_provider()?;

    print_step(3, 8, "Channels (How You Talk to AsteronIris)");
    let channels_config = setup_channels()?;

    print_step(4, 8, "Tunnel (Expose to Internet)");
    let tunnel_config = setup_tunnel()?;

    print_step(5, 8, "Tool Mode & Security");
    let (composio_config, secrets_config) = setup_tool_mode()?;

    print_step(6, 8, "Memory Configuration");
    let memory_config = setup_memory()?;

    print_step(7, 8, "Project Context (Personalize Your Agent)");
    let project_ctx = setup_project_context()?;

    print_step(8, 8, "Workspace Files");
    scaffold_workspace(&workspace_dir, &project_ctx)?;

    // ── Build config ──
    // Defaults: SQLite memory, supervised autonomy, workspace-scoped, native runtime
    let config = Config {
        workspace_dir: workspace_dir.clone(),
        config_path: config_path.clone(),
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        default_provider: Some(provider),
        default_model: Some(model),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        heartbeat: HeartbeatConfig::default(),
        channels_config,
        memory: memory_config, // User-selected memory backend
        tunnel: tunnel_config,
        gateway: crate::config::GatewayConfig::default(),
        composio: composio_config,
        secrets: secrets_config,
        browser: BrowserConfig::default(),
        persona: PersonaConfig::default(),
        identity: crate::config::IdentityConfig::default(),
    };

    println!(
        "  {} Security: {} | workspace-scoped",
        style("✓").green().bold(),
        style("Supervised").green()
    );
    println!(
        "  {} Memory: {} (auto-save: {})",
        style("✓").green().bold(),
        style(&config.memory.backend).green(),
        if config.memory.auto_save { "on" } else { "off" }
    );

    config.save()?;

    // ── Final summary ────────────────────────────────────────────
    print_summary(&config);

    // ── Offer to launch channels immediately ─────────────────────
    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
        || config.channels_config.email.is_some();

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!(
                "  {} Launch channels now? (connected channels → AI → reply)",
                style("🚀").cyan()
            ))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!(
                "  {} {}",
                style("⚡").cyan(),
                style("Starting channel server...").white().bold()
            );
            println!();
            // Signal to main.rs to call start_channels after wizard returns
            unsafe {
                std::env::set_var("ASTERONIRIS_AUTOSTART_CHANNELS", "1");
            }
        }
    }

    Ok(config)
}

/// Interactive repair flow: rerun channel setup only without redoing full onboarding.
pub fn run_channels_repair_wizard() -> Result<Config> {
    print_welcome_banner();
    println!(
        "  {}",
        style("Channels Repair — update channel tokens and allowlists only")
            .white()
            .bold()
    );
    println!();

    let mut config = Config::load_or_init()?;

    print_step(1, 1, "Channels (How You Talk to AsteronIris)");
    config.channels_config = setup_channels()?;
    config.save()?;

    println!();
    println!(
        "  {} Channel config saved: {}",
        style("✓").green().bold(),
        style(config.config_path.display()).green()
    );

    let has_channels = config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
        || config.channels_config.email.is_some();

    if has_channels && config.api_key.is_some() {
        let launch: bool = Confirm::new()
            .with_prompt(format!(
                "  {} Launch channels now? (connected channels → AI → reply)",
                style("🚀").cyan()
            ))
            .default(true)
            .interact()?;

        if launch {
            println!();
            println!(
                "  {} {}",
                style("⚡").cyan(),
                style("Starting channel server...").white().bold()
            );
            println!();
            // Signal to main.rs to call start_channels after wizard returns
            unsafe {
                std::env::set_var("ASTERONIRIS_AUTOSTART_CHANNELS", "1");
            }
        }
    }

    Ok(config)
}

// ── Quick setup (zero prompts) ───────────────────────────────────

/// Non-interactive setup: generates a sensible default config instantly.
/// Use `asteroniris onboard` or `asteroniris onboard --api-key sk-... --provider openrouter --memory sqlite`.
/// Use `asteroniris onboard --interactive` for the full wizard.
#[allow(clippy::too_many_lines)]
pub fn run_quick_setup(
    api_key: Option<&str>,
    provider: Option<&str>,
    memory_backend: Option<&str>,
) -> Result<Config> {
    print_welcome_banner();
    println!(
        "  {}",
        style("Quick Setup — generating config with sensible defaults...")
            .white()
            .bold()
    );
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

    // Create memory config based on backend choice
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
        tunnel: crate::config::TunnelConfig::default(),
        gateway: crate::config::GatewayConfig::default(),
        composio: ComposioConfig::default(),
        secrets: SecretsConfig::default(),
        browser: BrowserConfig::default(),
        persona: PersonaConfig::default(),
        identity: crate::config::IdentityConfig::default(),
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
        "  {} Workspace:  {}",
        style("✓").green().bold(),
        style(workspace_dir.display()).green()
    );
    println!(
        "  {} Provider:   {}",
        style("✓").green().bold(),
        style(&provider_name).green()
    );
    println!(
        "  {} Model:      {}",
        style("✓").green().bold(),
        style(&model).green()
    );
    println!(
        "  {} API Key:    {}",
        style("✓").green().bold(),
        if api_key.is_some() {
            style("set").green()
        } else {
            style("not set (use --api-key or edit config.toml)").yellow()
        }
    );
    println!(
        "  {} Security:   {}",
        style("✓").green().bold(),
        style("Supervised (workspace-scoped)").green()
    );
    println!(
        "  {} Memory:     {} (auto-save: {})",
        style("✓").green().bold(),
        style(&memory_backend_name).green(),
        if memory_backend_name == "none" {
            "off"
        } else {
            "on"
        }
    );
    println!(
        "  {} Secrets:    {}",
        style("✓").green().bold(),
        style("encrypted").green()
    );
    println!(
        "  {} Gateway:    {}",
        style("✓").green().bold(),
        style("pairing required (127.0.0.1:8080)").green()
    );
    println!(
        "  {} Tunnel:     {}",
        style("✓").green().bold(),
        style("none (local only)").dim()
    );
    println!(
        "  {} Composio:   {}",
        style("✓").green().bold(),
        style("disabled (sovereign mode)").dim()
    );
    println!();
    println!(
        "  {} {}",
        style("Config saved:").white().bold(),
        style(config_path.display()).green()
    );
    println!();
    println!("  {}", style("Next steps:").white().bold());
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

    Ok(config)
}
