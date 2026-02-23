use super::handlers::{
    handle_health, handle_pair, handle_webhook, handle_whatsapp_message, handle_whatsapp_verify,
};
use super::openai_compat_handler::handle_chat_completions;
use super::replay_guard::ReplayGuard;
use super::websocket::ws_handler;
use super::{AppState, MAX_BODY_SIZE, REQUEST_TIMEOUT_SECS};

use crate::config::Config;
use crate::core::memory::{self, Memory};
use crate::core::providers::{self, Provider};
use crate::core::tools;
use crate::core::tools::ToolRegistry;
use crate::security::auth::AuthBroker;
use crate::security::pairing::{PairingGuard, is_public_bind};
use crate::security::{EntityRateLimiter, PermissionStore, SecurityPolicy};
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

struct GatewayResources {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    mem: Arc<dyn Memory>,
    security: Arc<SecurityPolicy>,
    rate_limiter: Arc<EntityRateLimiter>,
    permission_store: Arc<PermissionStore>,
    registry: Arc<ToolRegistry>,
}

fn composio_key(config: &Config) -> Option<&str> {
    if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    }
}

fn build_gateway_resources(config: &Config, auth_broker: &AuthBroker) -> Result<GatewayResources> {
    let provider: Arc<dyn Provider> = Arc::from(
        providers::create_resilient_provider_with_oauth_recovery(
            config,
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

    #[cfg(feature = "taste")]
    let taste_provider: Option<Arc<dyn crate::core::providers::Provider>> = if config.taste.enabled
    {
        let provider_name = config.default_provider.as_deref().unwrap_or("anthropic");
        let api_key = auth_broker.resolve_provider_api_key(provider_name);
        providers::create_provider(provider_name, api_key.as_deref())
            .ok()
            .map(|p| Arc::from(p) as Arc<dyn crate::core::providers::Provider>)
    } else {
        None
    };
    #[cfg(not(feature = "taste"))]
    let taste_provider: Option<Arc<dyn crate::core::providers::Provider>> = None;

    let tools = tools::all_tools(
        &security,
        Arc::clone(&mem),
        composio_key(config),
        &config.browser,
        &config.tools,
        Some(&config.mcp),
        &config.taste,
        taste_provider,
    );
    let middleware = tools::default_middleware_chain();
    let mut registry = ToolRegistry::new(middleware);
    for tool in tools {
        registry.register(tool);
    }

    Ok(GatewayResources {
        provider,
        model,
        temperature,
        mem,
        security,
        rate_limiter,
        permission_store,
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
    whatsapp_channel: Option<Arc<WhatsAppChannel>>,
    whatsapp_app_secret: Option<Arc<str>>,
) -> AppState {
    AppState {
        provider: resources.provider,
        registry: resources.registry,
        rate_limiter: resources.rate_limiter,
        max_tool_loop_iterations: config.autonomy.max_tool_loop_iterations,
        permission_store: resources.permission_store,
        model: resources.model,
        temperature: resources.temperature,
        openai_compat_api_keys: None,
        mem: resources.mem,
        auto_save: config.memory.auto_save,
        webhook_secret,
        pairing,
        whatsapp: whatsapp_channel,
        whatsapp_app_secret,
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

    let auth_broker = AuthBroker::load_or_init(&config).context("load auth broker for gateway")?;

    let resources = build_gateway_resources(&config, &auth_broker)?;
    let webhook_secret = resolve_webhook_secret(&config);
    let whatsapp_channel = build_whatsapp_channel(&config);

    let whatsapp_app_secret = resolve_whatsapp_app_secret(&config);

    let pairing = Arc::new(PairingGuard::new(
        config.gateway.require_pairing,
        &config.gateway.paired_tokens,
        Some(config.gateway.token_ttl_secs),
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

    crate::runtime::diagnostics::health::mark_component_ok("gateway");

    let state = build_gateway_state(
        &config,
        resources,
        pairing,
        webhook_secret,
        whatsapp_channel,
        whatsapp_app_secret,
    );

    let app = build_app(state, &config.gateway.cors_origins);
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
    let tunnel = crate::runtime::tunnel::create_tunnel(&config.tunnel)
        .context("create tunnel for gateway")?;

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

fn build_app(state: AppState, cors_origins: &[String]) -> Router {
    let mut app = Router::new()
        .route("/health", get(handle_health))
        .route("/pair", post(handle_pair))
        .route("/webhook", post(handle_webhook))
        .route("/ws", get(ws_handler))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/whatsapp", get(handle_whatsapp_verify))
        .route("/whatsapp", post(handle_whatsapp_message))
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
