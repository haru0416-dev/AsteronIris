mod app;
pub mod state;
pub mod steps;
pub mod theme;
pub mod widgets;

use anyhow::Result;

use crate::config::{
    AutonomyConfig, BrowserConfig, ComposioConfig, Config, HeartbeatConfig, MemoryConfig,
    ObservabilityConfig, PersonaConfig, RuntimeConfig, SecretsConfig,
};
use crate::onboard::prompts::ProjectContext;
use crate::onboard::scaffold::scaffold_workspace;

/// Run the full-screen TUI wizard, returning a completed Config.
pub fn run_tui_wizard() -> Result<Config> {
    let wizard_state = app::run_app()?;

    if wizard_state.should_quit && !wizard_state.summary_confirmed {
        anyhow::bail!("Wizard cancelled by user");
    }

    build_config_from_state(&wizard_state)
}

fn build_config_from_state(state: &state::WizardState) -> Result<Config> {
    let workspace_dir = std::path::PathBuf::from(&state.workspace_dir);
    let config_path = std::path::PathBuf::from(&state.config_path);

    std::fs::create_dir_all(&workspace_dir)?;

    // Memory config
    let memory_backend = match state.memory_select.selected {
        1 => "markdown",
        2 => "none",
        _ => "sqlite",
    };
    let auto_save = state.memory_select.selected != 2 && state.memory_auto_save.value;

    let memory_config = MemoryConfig {
        backend: memory_backend.to_string(),
        auto_save,
        hygiene_enabled: memory_backend == "sqlite",
        archive_after_days: if memory_backend == "sqlite" { 7 } else { 0 },
        purge_after_days: if memory_backend == "sqlite" { 30 } else { 0 },
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
        embedding_cache_size: if memory_backend == "sqlite" { 10000 } else { 0 },
        chunk_max_tokens: 512,
    };

    // Tunnel config
    let tunnel_config = build_tunnel_config(state);

    // Composio + secrets
    let composio_config = if state.tool_mode_select.selected == 1 {
        let key = state.composio_api_key.value.trim().to_string();
        if key.is_empty() {
            ComposioConfig::default()
        } else {
            ComposioConfig {
                enabled: true,
                api_key: Some(key),
                ..ComposioConfig::default()
            }
        }
    } else {
        ComposioConfig::default()
    };

    let secrets_config = SecretsConfig {
        encrypt: state.encrypt_toggle.value,
    };

    // Build context and scaffold
    let communication_style = resolve_communication_style(state);

    let project_ctx = ProjectContext {
        user_name: state.context_name.value.clone(),
        timezone: resolve_timezone(state),
        agent_name: state.context_agent_name.value.clone(),
        communication_style,
    };

    scaffold_workspace(&workspace_dir, &project_ctx)?;

    let config = Config {
        workspace_dir,
        config_path,
        api_key: if state.selected_api_key.is_empty() {
            None
        } else {
            Some(state.selected_api_key.clone())
        },
        default_provider: Some(if state.selected_provider.is_empty() {
            "openrouter".into()
        } else {
            state.selected_provider.clone()
        }),
        default_model: Some(if state.selected_model.is_empty() {
            "anthropic/claude-sonnet-4-20250514".into()
        } else {
            state.selected_model.clone()
        }),
        default_temperature: 0.7,
        observability: ObservabilityConfig::default(),
        autonomy: AutonomyConfig::default(),
        runtime: RuntimeConfig::default(),
        reliability: crate::config::ReliabilityConfig::default(),
        heartbeat: HeartbeatConfig::default(),
        channels_config: state.channels_config.clone(),
        memory: memory_config,
        tunnel: tunnel_config,
        gateway: crate::config::GatewayConfig::default(),
        composio: composio_config,
        secrets: secrets_config,
        browser: BrowserConfig::default(),
        persona: PersonaConfig::default(),
        identity: crate::config::IdentityConfig::default(),
        locale: String::from("en"),
    };

    config.save()?;

    Ok(config)
}

fn build_tunnel_config(state: &state::WizardState) -> crate::config::TunnelConfig {
    use crate::config::schema::{
        CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, TailscaleTunnelConfig,
        TunnelConfig,
    };

    match state.tunnel_select.selected {
        1 => {
            let token = state.tunnel_token.value.trim().to_string();
            if token.is_empty() {
                TunnelConfig::default()
            } else {
                TunnelConfig {
                    provider: "cloudflare".into(),
                    cloudflare: Some(CloudflareTunnelConfig { token }),
                    ..TunnelConfig::default()
                }
            }
        }
        2 => TunnelConfig {
            provider: "tailscale".into(),
            tailscale: Some(TailscaleTunnelConfig {
                funnel: state.tunnel_funnel.value,
                hostname: None,
            }),
            ..TunnelConfig::default()
        },
        3 => {
            let auth_token = state.tunnel_token.value.trim().to_string();
            if auth_token.is_empty() {
                TunnelConfig::default()
            } else {
                let domain = state.tunnel_domain.value.trim().to_string();
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
            let cmd = state.tunnel_command.value.trim().to_string();
            if cmd.is_empty() {
                TunnelConfig::default()
            } else {
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
        _ => TunnelConfig::default(),
    }
}

fn resolve_timezone(state: &state::WizardState) -> String {
    let tz_idx = state.context_tz_select.selected;
    if tz_idx == state.context_tz_select.items.len() - 1 {
        // "Other" — use custom input
        let custom = state.context_tz_custom.value.trim().to_string();
        if custom.is_empty() {
            "UTC".into()
        } else {
            custom
        }
    } else {
        // Extract from the label (e.g. "US/Eastern (EST/EDT)" → "US/Eastern")
        state.context_tz_select.items[tz_idx]
            .split('(')
            .next()
            .unwrap_or("UTC")
            .trim()
            .to_string()
    }
}

fn resolve_communication_style(state: &state::WizardState) -> String {
    match state.context_style_select.selected {
        0 => "Be direct and concise. Skip pleasantries. Get to the point.".into(),
        1 => "Be friendly, human, and conversational. Show warmth and empathy while staying efficient.".into(),
        2 => "Be professional and polished. Stay calm, structured, and respectful.".into(),
        3 => "Be expressive and playful when appropriate. Use relevant emojis naturally (0-2 max).".into(),
        4 => "Be technical and detailed. Thorough explanations, code-first.".into(),
        5 => "Adapt to the situation. Default to warm and clear communication.".into(),
        _ => {
            // Custom
            let custom = state.context_style_custom.value.trim().to_string();
            if custom.is_empty() {
                "Be warm, natural, and clear.".into()
            } else {
                custom
            }
        }
    }
}
