use super::AppState;
use super::events::{ClientMessage, ServerMessage};
use crate::agent::{
    IntegrationRuntimeTurnOptions, IntegrationTurnParams, LoopStopReason,
    run_main_session_turn_for_runtime_with_policy,
};
use crate::security::policy::TenantPolicyContext;
use crate::tools::ExecutionContext;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use std::sync::Arc;

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
}

fn websocket_auth_response(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<(StatusCode, &'static str)> {
    let pairing_active = state.pairing.is_paired() || state.pairing.require_pairing();
    let api_keys = state.openai_compat_api_keys.as_deref().unwrap_or(&[]);
    let api_key_ok =
        bearer_token(headers).is_some_and(|token| api_keys.iter().any(|key| key == token));

    if pairing_active {
        let pairing_authenticated =
            bearer_token(headers).is_some_and(|token| state.pairing.is_authenticated(token));
        if pairing_authenticated || api_key_ok {
            None
        } else {
            Some((
                StatusCode::UNAUTHORIZED,
                "WebSocket upgrade requires authentication (pairing token or API key)",
            ))
        }
    } else if api_keys.is_empty() {
        Some((
            StatusCode::FORBIDDEN,
            "WebSocket disabled: no authentication is configured. Enable pairing or API keys.",
        ))
    } else if api_key_ok {
        None
    } else {
        Some((
            StatusCode::UNAUTHORIZED,
            "WebSocket upgrade requires a valid API key",
        ))
    }
}

pub async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if let Some(response) = websocket_auth_response(&state, &headers) {
        return response.into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, state))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let connected = ServerMessage::connected();
    if send_message(&mut socket, &connected).await.is_err() {
        return;
    }

    while let Some(result) = socket.recv().await {
        let message = match result {
            Ok(message) => message,
            Err(error) => {
                tracing::debug!("websocket receive error: {error}");
                break;
            }
        };

        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(client_message) => {
                    if handle_client_message(&mut socket, &state, client_message)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let server_message = ServerMessage::error(format!("invalid message: {error}"));
                    if send_message(&mut socket, &server_message).await.is_err() {
                        break;
                    }
                }
            },
            Message::Close(_) => break,
            Message::Ping(data) => {
                if socket.send(Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            _ => {}
        }
    }
}

async fn handle_client_message(
    socket: &mut WebSocket,
    state: &AppState,
    message: ClientMessage,
) -> Result<(), axum::Error> {
    match message {
        ClientMessage::Chat {
            session_id,
            message,
        } => {
            let typing = ServerMessage::Typing { agent: true };
            let _ = send_message(socket, &typing).await;

            let source_identifier = session_id.as_deref().unwrap_or("websocket");
            let entity_id = format!("gateway:{source_identifier}");
            let policy_context = TenantPolicyContext::disabled();
            let ctx = ExecutionContext {
                security: Arc::clone(&state.security),
                autonomy_level: state.security.autonomy,
                entity_id: entity_id.clone(),
                turn_number: 0,
                workspace_dir: state.security.workspace_dir.clone(),
                allowed_tools: None,
                rate_limiter: Arc::clone(&state.rate_limiter),
                tenant_context: policy_context.clone(),
            };

            match run_main_session_turn_for_runtime_with_policy(
                IntegrationTurnParams {
                    config: state.config.as_ref(),
                    security: state.security.as_ref(),
                    mem: Arc::clone(&state.mem),
                    answer_provider: state.provider.as_ref(),
                    reflect_provider: state.provider.as_ref(),
                    system_prompt: state.system_prompt.as_str(),
                    model_name: &state.model,
                    temperature: state.temperature,
                    entity_id: &entity_id,
                    policy_context,
                    user_message: &message,
                },
                IntegrationRuntimeTurnOptions {
                    registry: Arc::clone(&state.registry),
                    max_tool_iterations: state.max_tool_loop_iterations,
                    execution_context: ctx,
                    stream_sink: None,
                    conversation_history: &[],
                    hooks: &[],
                },
            )
            .await
            {
                Ok(result) => {
                    if let LoopStopReason::Error(error) = &result.stop_reason {
                        let server_message = ServerMessage::error(error);
                        send_message(socket, &server_message).await?;
                        return Ok(());
                    }
                    if matches!(result.stop_reason, LoopStopReason::MaxIterations) {
                        tracing::warn!(session_id = ?session_id, "websocket tool loop hit max iterations");
                    }
                    let reply = ServerMessage::chat_response(
                        session_id,
                        result.final_text,
                        None,
                        result.tokens_used,
                    );
                    send_message(socket, &reply).await?;
                }
                Err(error) => {
                    let server_message = ServerMessage::error(error.to_string());
                    send_message(socket, &server_message).await?;
                }
            }
        }
        ClientMessage::Typing { .. } => {}
        ClientMessage::Ping => {
            send_message(socket, &ServerMessage::Pong).await?;
        }
    }

    Ok(())
}

async fn send_message(socket: &mut WebSocket, message: &ServerMessage) -> Result<(), axum::Error> {
    let json = message.to_json();
    socket.send(Message::Text(json.into())).await
}
