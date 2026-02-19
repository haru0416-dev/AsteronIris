use crate::channels::Channel;
use crate::providers;
use crate::security::pairing::constant_time_eq;
use crate::util::truncate_with_ellipsis;
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};

use super::autosave::{
    gateway_runtime_policy_context, gateway_webhook_autosave_event,
    gateway_whatsapp_autosave_event, GATEWAY_AUTOSAVE_ENTITY_ID,
};
use super::defense::{
    apply_external_ingress_policy, policy_accounting_response, policy_violation_response,
    PolicyViolation,
};
use super::signature::verify_whatsapp_signature;
use super::{AppState, WebhookBody, WhatsAppVerifyQuery};

/// GET /health — always public (no secrets leaked)
pub(super) async fn handle_health(State(state): State<AppState>) -> impl IntoResponse {
    let body = serde_json::json!({
        "status": "ok",
        "paired": state.pairing.is_paired(),
        "runtime": crate::health::snapshot_json(),
    });
    Json(body)
}

/// POST /pair — exchange one-time code for bearer token
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
            tracing::info!("🔐 New client paired successfully");
            let body = serde_json::json!({
                "paired": true,
                "token": token,
                "message": "Save this token — use it as Authorization: Bearer <token>"
            });
            (StatusCode::OK, Json(body))
        }
        Ok(None) => {
            tracing::warn!("🔐 Pairing attempt with invalid code");
            let err = serde_json::json!({"error": "Invalid pairing code"});
            (StatusCode::FORBIDDEN, Json(err))
        }
        Err(lockout_secs) => {
            tracing::warn!(
                "🔐 Pairing locked out — too many failed attempts ({lockout_secs}s remaining)"
            );
            let err = serde_json::json!({
                "error": format!("Too many failed attempts. Try again in {lockout_secs}s."),
                "retry_after": lockout_secs
            });
            (StatusCode::TOO_MANY_REQUESTS, Json(err))
        }
    }
}

/// POST /webhook — main webhook endpoint
pub(super) async fn handle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<WebhookBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    // ── Bearer token auth (pairing) ──
    if state.pairing.require_pairing() {
        let auth = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = auth.strip_prefix("Bearer ").unwrap_or("");
        if !state.pairing.is_authenticated(token) {
            if let Some(response) =
                policy_violation_response(&state, PolicyViolation::MissingOrInvalidBearer)
            {
                return response;
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
                if let Some(response) = policy_violation_response(
                    &state,
                    PolicyViolation::MissingOrInvalidWebhookSecret,
                ) {
                    return response;
                }
            }
        }
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
        let policy_context = gateway_runtime_policy_context();
        if let Err(error) = policy_context.enforce_recall_scope(GATEWAY_AUTOSAVE_ENTITY_ID) {
            tracing::warn!(
                error,
                "gateway webhook autosave skipped due to policy context"
            );
        } else {
            let _ = state
                .mem
                .append_event(gateway_webhook_autosave_event(
                    ingress.persisted_summary.clone(),
                ))
                .await;
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

    match state
        .provider
        .chat(&ingress.model_input, &state.model, state.temperature)
        .await
    {
        Ok(response) => {
            let body = serde_json::json!({"response": response, "model": state.model});
            (StatusCode::OK, Json(body))
        }
        Err(e) => {
            tracing::error!(
                "Webhook provider error: {}",
                providers::sanitize_api_error(&e.to_string())
            );
            let err = serde_json::json!({"error": "LLM request failed"});
            (StatusCode::INTERNAL_SERVER_ERROR, Json(err))
        }
    }
}

/// GET /whatsapp — Meta webhook verification
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

/// POST /whatsapp — incoming message webhook
#[allow(clippy::too_many_lines)]
pub(super) async fn handle_whatsapp_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(ref wa) = state.whatsapp else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "WhatsApp not configured"})),
        );
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
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid signature"})),
            );
        }
    }

    // Parse JSON body
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Invalid JSON payload"})),
        );
    };

    // Parse messages from the webhook payload
    let messages = wa.parse_webhook_payload(&payload);

    if messages.is_empty() {
        // Acknowledge the webhook even if no messages (could be status updates)
        return (StatusCode::OK, Json(serde_json::json!({"status": "ok"})));
    }

    // Process each message
    for msg in &messages {
        tracing::info!(
            "WhatsApp message from {}: {}",
            msg.sender,
            truncate_with_ellipsis(&msg.content, 50)
        );
        let source = "gateway:whatsapp";

        // Auto-save to memory
        if state.auto_save {
            let ingress = apply_external_ingress_policy(source, &msg.content);
            let policy_context = gateway_runtime_policy_context();
            if let Err(error) = policy_context.enforce_recall_scope(GATEWAY_AUTOSAVE_ENTITY_ID) {
                tracing::warn!(
                    error,
                    "gateway whatsapp autosave skipped due to policy context"
                );
            } else {
                let _ = state
                    .mem
                    .append_event(gateway_whatsapp_autosave_event(
                        &msg.sender,
                        ingress.persisted_summary.clone(),
                    ))
                    .await;
            }

            if ingress.blocked {
                tracing::warn!(
                    source,
                    "blocked high-risk external content at whatsapp ingress"
                );
                let _ = wa
                    .send(
                        "I could not process that external content safely.",
                        &msg.sender,
                    )
                    .await;
                continue;
            }

            if let Err(policy_error) = state.security.consume_action_and_cost(0) {
                let _ = wa
                    .send(
                        "I cannot respond right now due to policy limits.",
                        &msg.sender,
                    )
                    .await;
                tracing::warn!("{policy_error}");
                continue;
            }

            // Call the LLM
            match state
                .provider
                .chat(&ingress.model_input, &state.model, state.temperature)
                .await
            {
                Ok(response) => {
                    // Send reply via WhatsApp
                    if let Err(e) = wa.send(&response, &msg.sender).await {
                        tracing::error!("Failed to send WhatsApp reply: {e}");
                    }
                }
                Err(e) => {
                    tracing::error!("LLM error for WhatsApp message: {e:#}");
                    let _ = wa
                        .send(
                            "Sorry, I couldn't process your message right now.",
                            &msg.sender,
                        )
                        .await;
                }
            }

            continue;
        }

        let ingress = apply_external_ingress_policy("gateway:whatsapp", &msg.content);
        if ingress.blocked {
            tracing::warn!("blocked high-risk external content at whatsapp ingress");
            let _ = wa
                .send(
                    "I could not process that external content safely.",
                    &msg.sender,
                )
                .await;
            continue;
        }

        if let Err(policy_error) = state.security.consume_action_and_cost(0) {
            let _ = wa
                .send(
                    "I cannot respond right now due to policy limits.",
                    &msg.sender,
                )
                .await;
            tracing::warn!("{policy_error}");
            continue;
        }

        match state
            .provider
            .chat(&ingress.model_input, &state.model, state.temperature)
            .await
        {
            Ok(response) => {
                if let Err(e) = wa.send(&response, &msg.sender).await {
                    tracing::error!("Failed to send WhatsApp reply: {e}");
                }
            }
            Err(e) => {
                tracing::error!("LLM error for WhatsApp message: {e:#}");
                let _ = wa
                    .send(
                        "Sorry, I couldn't process your message right now.",
                        &msg.sender,
                    )
                    .await;
            }
        }
    }

    // Acknowledge the webhook
    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}
