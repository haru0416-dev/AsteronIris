use super::handlers::{handle_health, handle_pair, handle_webhook};
#[cfg(feature = "whatsapp")]
use super::handlers::{handle_whatsapp_message, handle_whatsapp_verify};
use super::openai_compat_handler::handle_chat_completions;
use super::pairing::PairingGuard;
use super::replay_guard::ReplayGuard;
use super::websocket::ws_handler;
use super::{AppState, MAX_BODY_SIZE, REQUEST_TIMEOUT_SECS};

use crate::config::Config;
use crate::llm;
use crate::memory;
use crate::memory::Memory;
use crate::security::policy::{EntityRateLimiter, SecurityPolicy};
use crate::tools;
use crate::tools::ToolRegistry;
#[cfg(feature = "whatsapp")]
use crate::transport::channels::WhatsAppChannel;
use anyhow::{Context, Result};
use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

/// Returns true when the bind address is not a loopback address.
fn is_public_bind(host: &str) -> bool {
    !matches!(
        host,
        "127.0.0.1" | "localhost" | "::1" | "[::1]" | "0:0:0:0:0:0:0:1"
    )
}

/// Run the HTTP gateway using axum with proper HTTP/1.1 compliance.
pub async fn run_gateway(host: &str, port: u16, config: Arc<Config>) -> Result<()> {
    // ── Security: refuse public bind without tunnel or explicit opt-in ──
    if is_public_bind(host) && config.tunnel.provider == "none" && !config.gateway.allow_public_bind
    {
        anyhow::bail!(
            "Refusing to bind to {host} — gateway would be exposed to the internet.\n\
             Fix: use --host 127.0.0.1 (default), configure a tunnel, or set\n\
             [gateway] allow_public_bind = true in config.toml (NOT recommended)."
        );
    }

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("parse gateway bind address")?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("bind gateway socket")?;

    run_gateway_with_listener(host, listener, config).await
}

struct GatewayResources {
    provider: Arc<dyn llm::Provider>,
    model: String,
    temperature: f64,
    mem: Arc<dyn Memory>,
    security: Arc<SecurityPolicy>,
    rate_limiter: Arc<EntityRateLimiter>,
    registry: Arc<ToolRegistry>,
}

async fn build_gateway_resources(config: &Config) -> Result<GatewayResources> {
    let provider_name = config.default_provider.as_deref().unwrap_or("openrouter");
    let api_key = llm::factory::resolve_api_key(provider_name, None);
    let provider: Arc<dyn llm::Provider> = Arc::from(
        llm::create_resilient_provider_with_oauth_recovery(
            config,
            provider_name,
            &config.reliability,
            |name| llm::factory::resolve_api_key(name, None),
        )
        .context("create resilient LLM provider")?,
    );
    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
    let temperature = config.default_temperature;

    let memory_api_key = api_key; // TODO: dedicated memory API key resolution
    let mem: Arc<dyn Memory> = Arc::from(
        memory::factory::create_memory(
            &config.memory,
            &config.workspace_dir,
            memory_api_key.as_deref(),
        )
        .await
        .context("create memory backend for gateway")?,
    );

    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let rate_limiter = Arc::new(EntityRateLimiter::new(
        config.autonomy.max_actions_per_hour,
        config.autonomy.max_actions_per_entity_per_hour,
    ));

    let tool_list = tools::all_tools(Arc::clone(&mem));
    let mut registry = ToolRegistry::new(vec![]);
    for tool in tool_list {
        registry.register(tool);
    }

    Ok(GatewayResources {
        provider,
        model,
        temperature,
        mem,
        security,
        rate_limiter,
        registry: Arc::new(registry),
    })
}

fn resolve_webhook_secret(config: &Config) -> Option<Arc<str>> {
    config
        .channels_config
        .webhook
        .as_ref()
        .and_then(|webhook| webhook.secret.as_deref())
        .map(Arc::from)
}

#[cfg(feature = "whatsapp")]
fn build_whatsapp_channel(config: &Config) -> Option<Arc<WhatsAppChannel>> {
    config.channels_config.whatsapp.as_ref().map(|whatsapp| {
        Arc::new(WhatsAppChannel::new(
            whatsapp.access_token.clone(),
            whatsapp.phone_number_id.clone(),
            whatsapp.verify_token.clone(),
            whatsapp.allowed_numbers.clone(),
        ))
    })
}

fn build_gateway_state(
    config: &Config,
    resources: GatewayResources,
    pairing: Arc<PairingGuard>,
    webhook_secret: Option<Arc<str>>,
) -> AppState {
    AppState {
        provider: resources.provider,
        registry: resources.registry,
        rate_limiter: resources.rate_limiter,
        max_tool_loop_iterations: config.autonomy.max_tool_loop_iterations,
        model: resources.model,
        temperature: resources.temperature,
        openai_compat_api_keys: None,
        mem: resources.mem,
        auto_save: config.memory.auto_save,
        webhook_secret,
        pairing,
        #[cfg(feature = "whatsapp")]
        whatsapp: build_whatsapp_channel(config),
        #[cfg(feature = "whatsapp")]
        whatsapp_app_secret: resolve_whatsapp_app_secret(config),
        defense_mode: config.gateway.defense_mode,
        defense_kill_switch: config.gateway.defense_kill_switch,
        security: resources.security,
        replay_guard: Arc::new(ReplayGuard::new()),
    }
}

/// Run the HTTP gateway from a pre-bound listener.
pub async fn run_gateway_with_listener(
    host: &str,
    listener: tokio::net::TcpListener,
    config: Arc<Config>,
) -> Result<()> {
    let actual_port = listener
        .local_addr()
        .context("get gateway listener local address")?
        .port();
    let display_addr = format!("{host}:{actual_port}");

    let resources = build_gateway_resources(&config).await?;
    let webhook_secret = resolve_webhook_secret(&config);

    #[cfg(feature = "whatsapp")]
    let whatsapp_enabled = config.channels_config.whatsapp.is_some();
    #[cfg(not(feature = "whatsapp"))]
    let whatsapp_enabled = false;

    let pairing = Arc::new(PairingGuard::new(
        config.gateway.require_pairing,
        &config.gateway.paired_tokens,
        Some(config.gateway.token_ttl_secs),
    ));

    print_gateway_banner(
        &display_addr,
        whatsapp_enabled,
        &pairing,
        webhook_secret.is_some(),
    );

    let state = build_gateway_state(&config, resources, pairing, webhook_secret);

    let app = build_app(state, &config.gateway.cors_origins);
    axum::serve(listener, app)
        .await
        .context("serve HTTP gateway")?;

    Ok(())
}

// Priority: environment variable > config file.
#[cfg(feature = "whatsapp")]
fn resolve_whatsapp_app_secret(config: &Config) -> Option<Arc<str>> {
    std::env::var("ASTERONIRIS_WHATSAPP_APP_SECRET")
        .ok()
        .and_then(|secret| {
            let secret = secret.trim();
            (!secret.is_empty()).then(|| secret.to_owned())
        })
        .or_else(|| {
            config.channels_config.whatsapp.as_ref().and_then(|wa| {
                wa.app_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|secret| !secret.is_empty())
                    .map(ToOwned::to_owned)
            })
        })
        .map(Arc::from)
}

fn print_gateway_banner(
    display_addr: &str,
    whatsapp_enabled: bool,
    pairing: &PairingGuard,
    webhook_secret_enabled: bool,
) {
    println!("Gateway listening on {display_addr}");
    println!("  POST /pair");
    println!("  POST /webhook");
    println!("  GET  /ws -> WebSocket");
    if whatsapp_enabled {
        println!("  GET  /whatsapp");
        println!("  POST /whatsapp");
    }
    println!("  GET  /health");
    if let Some(code) = pairing.pairing_code() {
        println!();
        println!("  Pairing required:");
        println!("     {code}");
    } else if pairing.require_pairing() {
        println!("  Pairing active");
    } else {
        println!("  Pairing disabled");
    }
    if webhook_secret_enabled {
        println!("  Webhook secret enabled");
    }
}

fn build_app(state: AppState, cors_origins: &[String]) -> Router {
    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/pair", post(handle_pair))
        .route("/webhook", post(handle_webhook))
        .route("/ws", get(ws_handler))
        .route("/v1/chat/completions", post(handle_chat_completions));

    #[cfg(feature = "whatsapp")]
    let app = app
        .route("/whatsapp", get(handle_whatsapp_verify))
        .route("/whatsapp", post(handle_whatsapp_message));

    let mut app = app
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(REQUEST_TIMEOUT_SECS),
        ));

    if !cors_origins.is_empty() {
        let origins: Vec<_> = cors_origins.iter().filter_map(|o| o.parse().ok()).collect();
        app = app.layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                ]),
        );
    }

    app
}
