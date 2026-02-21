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
pub(crate) mod openai_compat_auth;
pub(crate) mod openai_compat_handler;
pub(crate) mod openai_compat_streaming;
pub(crate) mod openai_compat_types;
mod server;
mod signature;
mod websocket;

// Re-exported for integration tests (tests/persona/scope_regression.rs).
pub use server::run_gateway;
#[allow(unused_imports)]
pub use server::run_gateway_with_listener;
#[allow(unused_imports)]
pub use signature::verify_whatsapp_signature;

use crate::channels::WhatsAppChannel;
use crate::config::GatewayDefenseMode;
use crate::intelligence::memory::Memory;
use crate::intelligence::providers::Provider;
use crate::intelligence::tools::ToolRegistry;
use crate::security::pairing::PairingGuard;
use crate::security::{EntityRateLimiter, PermissionStore, SecurityPolicy};
use std::sync::Arc;

#[cfg(test)]
use axum::http::StatusCode;
#[cfg(test)]
use handlers::{handle_health, handle_webhook, handle_whatsapp_message, handle_whatsapp_verify};

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

#[cfg(test)]
mod tests;
