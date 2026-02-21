use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Chat {
        session_id: Option<String>,
        message: String,
    },
    Typing {
        session_id: Option<String>,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    ChatResponse {
        session_id: Option<String>,
        content: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    Typing {
        agent: bool,
    },
    Error {
        message: String,
    },
    Pong,
    Connected {
        version: String,
    },
}

impl ServerMessage {
    pub fn chat_response(
        session_id: Option<String>,
        content: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Self {
        Self::ChatResponse {
            session_id,
            content,
            input_tokens,
            output_tokens,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    pub fn connected() -> Self {
        Self::Connected {
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"type":"error","message":"serialization failed"}"#.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientMessage, ServerMessage};

    #[test]
    fn client_message_chat_roundtrip() {
        let original = ClientMessage::Chat {
            session_id: Some("session-1".to_string()),
            message: "hello".to_string(),
        };

        let json = serde_json::to_string(&original).unwrap();
        let decoded: ClientMessage = serde_json::from_str(&json).unwrap();

        assert!(matches!(
            decoded,
            ClientMessage::Chat {
                session_id: Some(session_id),
                message
            } if session_id == "session-1" && message == "hello"
        ));
    }

    #[test]
    fn client_message_ping_roundtrip() {
        let original = ClientMessage::Ping;

        let json = serde_json::to_string(&original).unwrap();
        let decoded: ClientMessage = serde_json::from_str(&json).unwrap();

        assert!(matches!(decoded, ClientMessage::Ping));
    }

    #[test]
    fn server_message_chat_response_serializes() {
        let message = ServerMessage::chat_response(
            Some("session-2".to_string()),
            "world".to_string(),
            Some(10),
            Some(20),
        );
        let value = serde_json::to_value(message).unwrap();

        assert_eq!(value["type"], "chat_response");
        assert_eq!(value["session_id"], "session-2");
        assert_eq!(value["content"], "world");
        assert_eq!(value["input_tokens"], 10);
        assert_eq!(value["output_tokens"], 20);
    }

    #[test]
    fn server_message_error_serializes() {
        let message = ServerMessage::error("boom");
        let value = serde_json::to_value(message).unwrap();

        assert_eq!(value["type"], "error");
        assert_eq!(value["message"], "boom");
    }

    #[test]
    fn server_message_connected_includes_version() {
        let message = ServerMessage::connected();
        let value = serde_json::to_value(message).unwrap();

        assert_eq!(value["type"], "connected");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn server_message_to_json_produces_valid_json() {
        let json = ServerMessage::Pong.to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["type"], "pong");
    }
}
