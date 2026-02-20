//! Axum-based HTTP gateway with proper HTTP/1.1 compliance, body limits, and timeouts.
//!
//! This module replaces the raw TCP implementation with axum for:
//! - Proper HTTP/1.1 parsing and compliance
//! - Content-Length validation (handled by hyper)
//! - Request body size limits (64KB max)
//! - Request timeouts (30s) to prevent slow-loris attacks
//! - Header sanitization (handled by axum/hyper)

mod autosave;
mod defense;
mod events;
mod handlers;
pub mod openai_compat;
mod signature;
mod websocket;

// Re-exported for integration tests (tests/persona/scope_regression.rs).
#[allow(unused_imports)]
pub use signature::verify_whatsapp_signature;

use handlers::{
    handle_health, handle_pair, handle_webhook, handle_whatsapp_message, handle_whatsapp_verify,
};
use websocket::ws_handler;

use crate::auth::AuthBroker;
use crate::channels::WhatsAppChannel;
use crate::config::{Config, GatewayDefenseMode};
use crate::memory::{self, Memory};
use crate::providers::{self, Provider};
use crate::security::pairing::{PairingGuard, is_public_bind};
use crate::security::{EntityRateLimiter, PermissionStore, SecurityPolicy};
use crate::tools;
use crate::tools::ToolRegistry;
use anyhow::{Context, Result};
use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

/// Maximum request body size (64KB) — prevents memory exhaustion
pub const MAX_BODY_SIZE: usize = 65_536;
/// Request timeout (30s) — prevents slow-loris attacks
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Shared state for all axum handlers
#[derive(Clone)]
pub struct AppState {
    pub provider: Arc<dyn Provider>,
    pub registry: Arc<ToolRegistry>,
    pub rate_limiter: Arc<EntityRateLimiter>,
    pub max_tool_loop_iterations: u32,
    pub permission_store: Arc<PermissionStore>,
    pub model: String,
    pub temperature: f64,
    pub openai_compat_api_keys: Option<Vec<String>>,
    pub mem: Arc<dyn Memory>,
    pub auto_save: bool,
    pub webhook_secret: Option<Arc<str>>,
    pub pairing: Arc<PairingGuard>,
    pub whatsapp: Option<Arc<WhatsAppChannel>>,
    /// `WhatsApp` app secret for webhook signature verification (`X-Hub-Signature-256`)
    pub whatsapp_app_secret: Option<Arc<str>>,
    pub defense_mode: GatewayDefenseMode,
    pub defense_kill_switch: bool,
    pub security: Arc<SecurityPolicy>,
}

/// Webhook request body
#[derive(serde::Deserialize)]
pub struct WebhookBody {
    pub message: String,
}

/// `WhatsApp` verification query params
#[derive(serde::Deserialize)]
pub struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.mode")]
    pub mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub challenge: Option<String>,
}

/// Run the HTTP gateway using axum with proper HTTP/1.1 compliance.
pub async fn run_gateway(host: &str, port: u16, config: Arc<Config>) -> Result<()> {
    // ── Security: refuse public bind without tunnel or explicit opt-in ──
    if is_public_bind(host) && config.tunnel.provider == "none" && !config.gateway.allow_public_bind
    {
        anyhow::bail!(
            "🛑 Refusing to bind to {host} — gateway would be exposed to the internet.\n\
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

/// Run the HTTP gateway from a pre-bound listener.
#[allow(clippy::too_many_lines)]
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

    let auth_broker = AuthBroker::load_or_init(&config).context("load auth broker for gateway")?;

    let provider: Arc<dyn Provider> = Arc::from(
        providers::create_resilient_provider_with_oauth_recovery(
            &config,
            config.default_provider.as_deref().unwrap_or("openrouter"),
            &config.reliability,
            |name| auth_broker.resolve_provider_api_key(name),
        )
        .context("create resilient LLM provider")?,
    );
    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
    let temperature = config.default_temperature;
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(
        memory::create_memory(
            &config.memory,
            &config.workspace_dir,
            memory_api_key.as_deref(),
        )
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
    let permission_store = Arc::new(PermissionStore::load(&config.workspace_dir));
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools = tools::all_tools(
        &security,
        Arc::clone(&mem),
        composio_key,
        &config.browser,
        &config.tools,
    );
    let middleware = tools::default_middleware_chain();
    let mut registry = ToolRegistry::new(middleware);
    for tool in tools {
        registry.register(tool);
    }

    let webhook_secret: Option<Arc<str>> = config
        .channels_config
        .webhook
        .as_ref()
        .and_then(|w| w.secret.as_deref())
        .map(Arc::from);

    let whatsapp_channel: Option<Arc<WhatsAppChannel>> =
        config.channels_config.whatsapp.as_ref().map(|wa| {
            Arc::new(WhatsAppChannel::new(
                wa.access_token.clone(),
                wa.phone_number_id.clone(),
                wa.verify_token.clone(),
                wa.allowed_numbers.clone(),
            ))
        });

    let whatsapp_app_secret = resolve_whatsapp_app_secret(&config);

    let pairing = Arc::new(PairingGuard::new(
        config.gateway.require_pairing,
        &config.gateway.paired_tokens,
    ));

    let tunnel_url = start_tunnel(&config, host, actual_port)
        .await
        .context("start gateway tunnel")?;

    print_gateway_banner(
        &display_addr,
        tunnel_url.as_deref(),
        whatsapp_channel.is_some(),
        &pairing,
        webhook_secret.is_some(),
    );

    crate::health::mark_component_ok("gateway");

    let state = AppState {
        provider,
        registry: Arc::new(registry),
        rate_limiter,
        max_tool_loop_iterations: config.autonomy.max_tool_loop_iterations,
        permission_store,
        model,
        temperature,
        openai_compat_api_keys: None,
        mem,
        auto_save: config.memory.auto_save,
        webhook_secret,
        pairing,
        whatsapp: whatsapp_channel,
        whatsapp_app_secret,
        defense_mode: config.gateway.defense_mode,
        defense_kill_switch: config.gateway.defense_kill_switch,
        security,
    };

    let app = build_app(state);
    axum::serve(listener, app)
        .await
        .context("serve HTTP gateway")?;

    Ok(())
}

// Priority: environment variable > config file.
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

async fn start_tunnel(config: &Config, host: &str, port: u16) -> Result<Option<String>> {
    let tunnel =
        crate::tunnel::create_tunnel(&config.tunnel).context("create tunnel for gateway")?;

    let Some(ref tun) = tunnel else {
        return Ok(None);
    };

    println!("› {}", t!("gateway.tunnel_starting", name = tun.name()));
    match tun.start(host, port).await {
        Ok(url) => {
            println!("✓ {}", t!("gateway.tunnel_active", url = url));
            Ok(Some(url))
        }
        Err(e) => {
            println!("! {}", t!("gateway.tunnel_failed", error = e));
            println!("   {}", t!("gateway.tunnel_fallback"));
            Ok(None)
        }
    }
}

fn print_gateway_banner(
    display_addr: &str,
    tunnel_url: Option<&str>,
    whatsapp_enabled: bool,
    pairing: &PairingGuard,
    webhook_secret_enabled: bool,
) {
    println!("◆ {}", t!("gateway.listening", addr = display_addr));
    if let Some(url) = tunnel_url {
        println!("  › {}", t!("gateway.public_url", url = url));
    }
    println!("  {}", t!("gateway.route_pair"));
    println!("  {}", t!("gateway.route_webhook"));
    println!("  GET /ws → WebSocket");
    if whatsapp_enabled {
        println!("  {}", t!("gateway.route_whatsapp_get"));
        println!("  {}", t!("gateway.route_whatsapp_post"));
    }
    println!("  {}", t!("gateway.route_health"));
    if let Some(code) = pairing.pairing_code() {
        println!();
        println!("  ✓ {}", t!("gateway.pairing_required"));
        println!("     ┌──────────────┐");
        println!("     │  {code}  │");
        println!("     └──────────────┘");
        println!("     {}", t!("gateway.pairing_send", code = code));
    } else if pairing.require_pairing() {
        println!("  ✓ {}", t!("gateway.pairing_active"));
    } else {
        println!("  ! {}", t!("gateway.pairing_disabled"));
    }
    if webhook_secret_enabled {
        println!("  ✓ {}", t!("gateway.webhook_secret_enabled"));
    }
    println!("  {}\n", t!("gateway.stop_hint"));
}

fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handle_health))
        .route("/pair", post(handle_pair))
        .route("/webhook", post(handle_webhook))
        .route("/ws", get(ws_handler))
        .route(
            "/v1/chat/completions",
            post(openai_compat::handle_chat_completions),
        )
        .route("/whatsapp", get(handle_whatsapp_verify))
        .route("/whatsapp", post(handle_whatsapp_message))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(REQUEST_TIMEOUT_SECS),
        ))
}

#[cfg(test)]
mod tests;
