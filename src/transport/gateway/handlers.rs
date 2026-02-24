use crate::agent::tool_loop::{LoopStopReason, ToolLoop, ToolLoopRunParams};
use crate::llm;
use crate::persona::channel_person_entity_id;
use crate::security::policy::TenantPolicyContext;
use crate::tools::ExecutionContext;
#[cfg(feature = "whatsapp")]
use crate::transport::channels::{Channel, WhatsAppChannel};
#[cfg(feature = "whatsapp")]
use crate::utils::text::truncate_with_ellipsis;
#[cfg(feature = "whatsapp")]
use axum::{body::Bytes, extract::Query};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Json},
};
use std::sync::Arc;

#[cfg(feature = "whatsapp")]
use super::WhatsAppVerifyQuery;
use super::autosave::gateway_webhook_autosave_event;
#[cfg(feature = "whatsapp")]
use super::autosave::gateway_whatsapp_autosave_event;
use super::autosave::{gateway_autosave_entity_id, gateway_runtime_policy_context};
use super::defense::{
    PolicyViolation, apply_external_ingress_policy, must_enforce_auth_violation,
    policy_accounting_response, policy_violation_response,
};
#[cfg(feature = "whatsapp")]
use super::signature::verify_whatsapp_signature;
use super::{AppState, WebhookBody};

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
}

fn log_tool_loop_stop(source: &str, stop_reason: &LoopStopReason, iterations: u32) {
    match stop_reason {
        LoopStopReason::Completed => {}
        LoopStopReason::MaxIterations => {
            tracing::warn!(source, iterations, "tool loop hit max iterations");
        }
        LoopStopReason::RateLimited => {
            tracing::warn!(source, "tool loop halted by rate limiter");
        }
        LoopStopReason::ApprovalDenied => {
            tracing::warn!(source, "tool loop halted pending approval");
        }
        LoopStopReason::HookBlocked(reason) => {
            tracing::warn!(source, reason, "tool loop halted by prompt hook");
        }
        LoopStopReason::Error(error) => {
            tracing::warn!(source, error = %error, "tool loop ended with provider error");
        }
    }
}

async fn run_gateway_tool_loop(
    state: &AppState,
    system_prompt: Option<&str>,
    user_message: &str,
    model: &str,
    temperature: f64,
    source_identifier: &str,
) -> anyhow::Result<crate::agent::tool_loop::ToolLoopResult> {
    let tool_loop = ToolLoop::new(Arc::clone(&state.registry), state.max_tool_loop_iterations);
    let full_prompt = system_prompt.unwrap_or_default();
    let ctx = ExecutionContext {
        security: Arc::clone(&state.security),
        autonomy_level: state.security.autonomy,
        entity_id: channel_person_entity_id("gateway", source_identifier),
        turn_number: 0,
        workspace_dir: state.security.workspace_dir.clone(),
        allowed_tools: None,
        rate_limiter: Arc::clone(&state.rate_limiter),
        tenant_context: TenantPolicyContext::disabled(),
    };
    let result = tool_loop
        .run(ToolLoopRunParams {
            provider: state.provider.as_ref(),
            system_prompt: full_prompt,
            user_message,
            image_content: &[],
            model,
            temperature,
            ctx: &ctx,
            stream_sink: None,
            conversation_history: &[],
            hooks: &[],
        })
        .await?;
    if let LoopStopReason::Error(error) = &result.stop_reason {
        anyhow::bail!("tool loop failed: {error}");
    }
    Ok(result)
}

#[cfg(feature = "whatsapp")]
fn whatsapp_not_configured_response() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "WhatsApp not configured"})),
    )
}

#[cfg(feature = "whatsapp")]
fn invalid_whatsapp_signature_response() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "Invalid signature"})),
    )
}

#[cfg(feature = "whatsapp")]
fn invalid_whatsapp_payload_response() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "Invalid JSON payload"})),
    )
}

#[cfg(feature = "whatsapp")]
fn whatsapp_ack_response() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

#[cfg(feature = "whatsapp")]
async fn send_whatsapp_reply_or_log(wa: &WhatsAppChannel, sender: &str, message: &str) {
    if let Err(error) = wa.send_chunked(message, sender).await {
        tracing::error!("Failed to send WhatsApp reply: {error}");
    }
}

#[cfg(feature = "whatsapp")]
async fn process_whatsapp_message(
    state: &AppState,
    wa: &WhatsAppChannel,
    sender: &str,
    content: &str,
) {
    let source = "gateway:whatsapp";
    let ingress = apply_external_ingress_policy(source, content);

    if state.auto_save {
        let autosave_entity_id = gateway_autosave_entity_id(sender);
        let policy_context = gateway_runtime_policy_context();
        if let Err(error) = policy_context.enforce_recall_scope(&autosave_entity_id) {
            tracing::warn!(
                error,
                "gateway whatsapp autosave skipped due to policy context"
            );
        } else {
            let event = gateway_whatsapp_autosave_event(
                &autosave_entity_id,
                sender,
                ingress.persisted_summary.clone(),
            );
            // TODO: enforce_external_autosave_write_policy once writeback_guard is ported
            if let Err(error) = state.mem.append_event(event).await {
                tracing::warn!(%error, "failed to autosave whatsapp event");
            }
        }
    }

    if ingress.blocked {
        tracing::warn!(
            source,
            "blocked high-risk external content at whatsapp ingress"
        );
        if let Err(error) = wa
            .send_chunked("I could not process that external content safely.", sender)
            .await
        {
            tracing::warn!(%error, "failed to send whatsapp safety block reply");
        }
        return;
    }

    if let Err(policy_error) = state.security.consume_action_and_cost(0) {
        if let Err(error) = wa
            .send_chunked("I cannot respond right now due to policy limits.", sender)
            .await
        {
            tracing::warn!(%error, "failed to send whatsapp policy limit reply");
        }
        tracing::warn!("{policy_error}");
        return;
    }

    match run_gateway_tool_loop(
        state,
        None,
        &ingress.model_input,
        &state.model,
        state.temperature,
        sender,
    )
    .await
    {
        Ok(result) => {
            log_tool_loop_stop("gateway:whatsapp", &result.stop_reason, result.iterations);
            send_whatsapp_reply_or_log(wa, sender, &result.final_text).await;
        }
        Err(error) => {
            tracing::error!("LLM error for WhatsApp message: {error:#}");
            if let Err(error) = wa
                .send_chunked("Sorry, I couldn't process your message right now.", sender)
                .await
            {
                tracing::warn!(%error, "failed to send whatsapp error reply");
            }
        }
    }
}

/// GET /health -- always public (no secrets leaked)
pub(super) async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let body = serde_json::json!({
        "status": "ok",
        "paired": state.pairing.is_paired(),
    });
    Json(body)
}

/// POST /pair -- exchange one-time code for bearer token
pub(super) async fn handle_pair(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let code = headers
        .get("X-Pairing-Code")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    match state.pairing.try_pair(code) {
        Ok(Some(token)) => {
            tracing::info!("New client paired successfully");
            let body = serde_json::json!({
                "paired": true,
                "token": token,
                "message": "Save this token — use it as Authorization: Bearer <token>"
            });
            (StatusCode::OK, Json(body))
        }
        Ok(None) => {
            tracing::warn!("Pairing attempt with invalid code");
            let err = serde_json::json!({"error": "Invalid pairing code"});
            (StatusCode::FORBIDDEN, Json(err))
        }
        Err(lockout_secs) => {
            tracing::warn!(
                "Pairing locked out — too many failed attempts ({lockout_secs}s remaining)"
            );
            let err = serde_json::json!({
                "error": format!("Too many failed attempts. Try again in {lockout_secs}s."),
                "retry_after": lockout_secs
            });
            (StatusCode::TOO_MANY_REQUESTS, Json(err))
        }
    }
}

/// Constant-time equality comparison for secret tokens.
fn constant_time_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

/// POST /webhook -- main webhook endpoint
#[allow(clippy::too_many_lines)]
pub(super) async fn handle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<WebhookBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    // ── Bearer token auth ──
    // Always check bearer token when paired tokens exist, regardless of
    // whether `require_pairing` is enabled.  Pairing controls the initial
    // enrollment flow, NOT whether runtime endpoints need authentication.
    if state.pairing.is_paired() {
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            let violation = PolicyViolation::MissingOrInvalidBearer;
            if must_enforce_auth_violation(&state, violation) {
                return violation.enforce_response();
            }
        }
    }

    // ── Webhook secret auth (optional, additional layer) ──
    if let Some(ref secret) = state.webhook_secret {
        let header_val = headers
            .get("X-Webhook-Secret")
            .and_then(|v| v.to_str().ok());
        match header_val {
            Some(val) if constant_time_eq(val, secret.as_ref()) => {}
            _ => {
                let violation = PolicyViolation::MissingOrInvalidWebhookSecret;
                if must_enforce_auth_violation(&state, violation) {
                    return violation.enforce_response();
                }
            }
        }
    }

    // ── Reject if no authentication mechanism is configured at all ──
    // Prevents accidental unauthenticated access when pairing is disabled
    // and no webhook secret is set.
    if !state.pairing.is_paired()
        && state.webhook_secret.is_none()
        && let Some(response) = policy_violation_response(&state, PolicyViolation::NoAuthConfigured)
    {
        return response;
    }

    // ── Parse body ──
    let Json(webhook_body) = match body {
        Ok(b) => b,
        Err(e) => {
            let err = serde_json::json!({
                "error": format!("Invalid JSON: {e}. Expected: {{\"message\": \"...\"}}")
            });
            return (StatusCode::BAD_REQUEST, Json(err));
        }
    };

    let source = "gateway:webhook";
    let ingress = apply_external_ingress_policy(source, &webhook_body.message);

    if state.auto_save {
        let source_identifier = bearer_token(&headers).unwrap_or("anonymous");
        let autosave_entity_id = gateway_autosave_entity_id(source_identifier);
        let policy_context = gateway_runtime_policy_context();
        if let Err(error) = policy_context.enforce_recall_scope(&autosave_entity_id) {
            tracing::warn!(
                error,
                "gateway webhook autosave skipped due to policy context"
            );
        } else {
            let event = gateway_webhook_autosave_event(
                &autosave_entity_id,
                ingress.persisted_summary.clone(),
            );
            // TODO: enforce_external_autosave_write_policy once writeback_guard is ported
            if let Err(error) = state.mem.append_event(event).await {
                tracing::warn!(%error, "failed to autosave webhook event");
            }
        }
    }

    if ingress.blocked {
        tracing::warn!(
            source,
            "blocked high-risk external content at gateway webhook ingress"
        );
        let err = serde_json::json!({"error": "External content blocked by safety policy"});
        return (StatusCode::BAD_REQUEST, Json(err));
    }

    if let Err(policy_error) = state.security.consume_action_and_cost(0) {
        return policy_accounting_response(policy_error);
    }

    let source_identifier = bearer_token(&headers).unwrap_or("anonymous");
    match run_gateway_tool_loop(
        &state,
        None,
        &ingress.model_input,
        &state.model,
        state.temperature,
        source_identifier,
    )
    .await
    {
        Ok(result) => {
            log_tool_loop_stop("gateway:webhook", &result.stop_reason, result.iterations);
            let body = serde_json::json!({"response": result.final_text, "model": state.model});
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            tracing::error!(
                "Webhook provider error: {}",
                llm::sanitize_api_error(&e.to_string())
            );
            let err = serde_json::json!({"error": "LLM request failed"});
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err))
        }
    }
}

/// GET /whatsapp -- Meta webhook verification
#[cfg(feature = "whatsapp")]
pub(super) async fn handle_whatsapp_verify(
    State(state): State<AppState>,
    Query(params): Query<WhatsAppVerifyQuery>,
) -> impl IntoResponse {
    let Some(ref wa) = state.whatsapp else {
        return (StatusCode::NOT_FOUND, "WhatsApp not configured".to_string());
    };

    // Verify the token matches (constant-time comparison to prevent timing attacks)
    let token_matches = params
        .verify_token
        .as_deref()
        .is_some_and(|t| constant_time_eq(t, wa.verify_token()));
    if params.mode.as_deref() == Some("subscribe") && token_matches {
        if let Some(ch) = params.challenge {
            tracing::info!("WhatsApp webhook verified successfully");
            return (StatusCode::OK, ch);
        }
        return (StatusCode::BAD_REQUEST, "Missing hub.challenge".to_string());
    }

    tracing::warn!("WhatsApp webhook verification failed — token mismatch");
    (StatusCode::FORBIDDEN, "Forbidden".to_string())
}

/// POST /whatsapp -- incoming message webhook
#[cfg(feature = "whatsapp")]
pub(super) async fn handle_whatsapp_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref wa) = state.whatsapp else {
        return whatsapp_not_configured_response();
    };

    // ── Security: Verify X-Hub-Signature-256 if app_secret is configured ──
    if let Some(ref app_secret) = state.whatsapp_app_secret {
        let signature = headers
            .get("X-Hub-Signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !verify_whatsapp_signature(app_secret, &body, signature) {
            tracing::warn!(
                "WhatsApp webhook signature verification failed (signature: {})",
                if signature.is_empty() {
                    "missing"
                } else {
                    "invalid"
                }
            );
            return invalid_whatsapp_signature_response();
        }
    }

    // ── Replay protection: check if we've seen this body before ──
    if !state.replay_guard.check_and_record(&body) {
        tracing::warn!("WhatsApp webhook replay detected");
        return whatsapp_ack_response();
    }

    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return invalid_whatsapp_payload_response();
    };

    let messages = wa.parse_webhook_payload(&payload);

    if messages.is_empty() {
        // Acknowledge the webhook even if no messages (could be status updates)
        return whatsapp_ack_response();
    }

    for msg in &messages {
        tracing::info!(
            "WhatsApp message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );
        process_whatsapp_message(&state, wa, &msg.sender, &msg.content).await;
    }

    // Acknowledge the webhook
    whatsapp_ack_response()
}
