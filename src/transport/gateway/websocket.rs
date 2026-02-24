use super::AppState;
use super::events::{ClientMessage, ServerMessage};
use crate::agent::tool_loop::{LoopStopReason, ToolLoop, ToolLoopRunParams};
use crate::security::policy::TenantPolicyContext;
use crate::tools::ExecutionContext;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use std::sync::Arc;

pub async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if state.pairing.is_paired() || state.pairing.require_pairing() {
        let token = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|raw| raw.strip_prefix("Bearer "));

        let authenticated = token.is_some_and(|t| state.pairing.is_authenticated(t));

        if !authenticated {
            return (
                StatusCode::UNAUTHORIZED,
                "WebSocket upgrade requires authentication",
            )
                .into_response();
        }
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

            let tool_loop =
                ToolLoop::new(Arc::clone(&state.registry), state.max_tool_loop_iterations);
            let source_identifier = session_id.as_deref().unwrap_or("websocket");
            let ctx = ExecutionContext {
                security: Arc::clone(&state.security),
                autonomy_level: state.security.autonomy,
                entity_id: format!("gateway:{source_identifier}"),
                turn_number: 0,
                workspace_dir: state.security.workspace_dir.clone(),
                allowed_tools: None,
                rate_limiter: Arc::clone(&state.rate_limiter),
                tenant_context: TenantPolicyContext::disabled(),
            };

            match tool_loop
                .run(ToolLoopRunParams {
                    provider: state.provider.as_ref(),
                    system_prompt: "",
                    user_message: &message,
                    image_content: &[],
                    model: &state.model,
                    temperature: state.temperature,
                    ctx: &ctx,
                    stream_sink: None,
                    conversation_history: &[],
                    hooks: &[],
                })
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
