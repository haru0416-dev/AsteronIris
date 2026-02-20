use super::AppState;
use super::events::{ClientMessage, ServerMessage};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
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

            match state
                .provider
                .chat_with_system_full(None, &message, &state.model, state.temperature)
                .await
            {
                Ok(response) => {
                    let reply = ServerMessage::chat_response(
                        session_id,
                        response.text,
                        response.input_tokens,
                        response.output_tokens,
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
