use super::*;
use crate::core::providers::Provider;

#[test]
fn creates_with_key() {
    let p = OpenAiProvider::new(Some("sk-proj-abc123"));
    assert_eq!(
        p.cached_auth_header.as_deref(),
        Some("Bearer sk-proj-abc123")
    );
}

#[test]
fn creates_without_key() {
    let p = OpenAiProvider::new(None);
    assert!(p.cached_auth_header.is_none());
}

#[test]
fn creates_with_empty_key() {
    let p = OpenAiProvider::new(Some(""));
    assert_eq!(p.cached_auth_header.as_deref(), Some("Bearer "));
}

#[tokio::test]
async fn chat_fails_without_key() {
    let p = OpenAiProvider::new(None);
    let result = p.chat_with_system(None, "hello", "gpt-4o", 0.7).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("API key not set"));
}

#[tokio::test]
async fn chat_with_system_fails_without_key() {
    let p = OpenAiProvider::new(None);
    let result = p
        .chat_with_system(Some("You are AsteronIris"), "test", "gpt-4o", 0.5)
        .await;
    assert!(result.is_err());
}

#[test]
fn request_serializes_with_system_message() {
    let req = OpenAiProvider::build_request(Some("You are AsteronIris"), "hello", "gpt-4o", 0.7);
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"role\":\"system\""));
    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("gpt-4o"));
}

#[test]
fn request_serializes_without_system() {
    let req = OpenAiProvider::build_request(None, "hello", "gpt-4o", 0.0);
    let json = serde_json::to_string(&req).unwrap();
    assert!(!json.contains("system"));
    assert!(json.contains("\"temperature\":0.0"));
    assert!(!json.contains("\"tools\":"));
}

#[test]
fn response_deserializes_single_choice() {
    let json = r#"{"choices":[{"message":{"content":"Hi!"}}]}"#;
    let resp: ChatResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("Hi!"));
}

#[test]
fn response_deserializes_empty_choices() {
    let json = r#"{"choices":[]}"#;
    let resp: ChatResponse = serde_json::from_str(json).unwrap();
    assert!(resp.choices.is_empty());
}

#[test]
fn response_deserializes_multiple_choices() {
    let json = r#"{"choices":[{"message":{"content":"A"}},{"message":{"content":"B"}}]}"#;
    let resp: ChatResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.choices.len(), 2);
    assert_eq!(resp.choices[0].message.content.as_deref(), Some("A"));
}

#[test]
fn response_with_unicode() {
    let json = r#"{"choices":[{"message":{"content":"こんにちは 🦀"}}]}"#;
    let resp: ChatResponse = serde_json::from_str(json).unwrap();
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("こんにちは 🦀")
    );
}

#[test]
fn response_with_long_content() {
    let long = "x".repeat(100_000);
    let json = format!(r#"{{"choices":[{{"message":{{"content":"{long}"}}}}]}}"#);
    let resp: ChatResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(
        resp.choices[0].message.content.as_deref().map(str::len),
        Some(100_000)
    );
}

#[test]
fn tools_request_serializes_in_openai_function_format() {
    let messages = vec![ProviderMessage {
        role: MessageRole::User,
        content: vec![ContentBlock::Text {
            text: "list files".to_string(),
        }],
    }];
    let tools = vec![ToolSpec {
        name: "shell".to_string(),
        description: "Execute a shell command".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"}
            },
            "required": ["command"]
        }),
    }];

    let req = OpenAiProvider::build_tools_request(None, &messages, &tools, "gpt-4o", 0.2);
    let json = serde_json::to_value(&req).unwrap();

    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tools"][0]["function"]["name"], "shell");
    assert_eq!(
        json["tools"][0]["function"]["description"],
        "Execute a shell command"
    );
    assert_eq!(json["tools"][0]["function"]["parameters"]["type"], "object");
}

#[test]
fn request_without_tools_omits_tools_field() {
    let req = OpenAiProvider::build_tools_request(None, &[], &[], "gpt-4o", 0.1);
    let json = serde_json::to_value(&req).unwrap();

    assert!(json.get("tools").is_none());
}

#[test]
fn response_tool_calls_deserialize_and_parse_to_content_blocks() {
    let json = r#"{
        "choices": [{
            "message": {
                "content": null,
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "shell",
                        "arguments": "{\"command\":\"ls\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }]
    }"#;
    let resp: ChatResponse = serde_json::from_str(json).unwrap();
    let blocks =
        OpenAiProvider::parse_tool_calls(resp.choices[0].message.tool_calls.clone()).unwrap();

    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        ContentBlock::ToolUse { id, name, input } => {
            assert_eq!(id, "call_abc123");
            assert_eq!(name, "shell");
            assert_eq!(input, &serde_json::json!({"command": "ls"}));
        }
        _ => panic!("expected tool use block"),
    }
}

#[test]
fn finish_reason_mapping_tool_calls_and_stop() {
    assert_eq!(
        OpenAiProvider::map_finish_reason(Some("tool_calls")),
        StopReason::ToolUse
    );
    assert_eq!(
        OpenAiProvider::map_finish_reason(Some("stop")),
        StopReason::EndTurn
    );
}

#[test]
fn map_provider_message_handles_image_block() {
    let msg = ProviderMessage {
        role: MessageRole::User,
        content: vec![
            ContentBlock::Text {
                text: "What's this?".to_string(),
            },
            ContentBlock::Image {
                source: ImageSource::base64("image/jpeg", "abc123"),
            },
        ],
    };
    let messages = OpenAiProvider::map_provider_message(&msg);
    assert_eq!(messages.len(), 1);
    let json = serde_json::to_value(&messages[0]).unwrap();
    let content = json["content"].as_array().expect("content should be array");
    assert_eq!(content.len(), 2);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "image_url");
    assert!(
        content[1]["image_url"]["url"]
            .as_str()
            .expect("image url should be string")
            .starts_with("data:image/jpeg;base64,")
    );
}

#[test]
fn supports_tool_calling_returns_true() {
    let provider = OpenAiProvider::new(Some("sk-test"));
    assert!(provider.supports_tool_calling());
}

#[test]
fn supports_vision_returns_true() {
    let provider = OpenAiProvider::new(Some("test-key"));
    assert!(provider.supports_vision());
}

#[test]
fn parse_sse_data_lines_basic() {
    let chunk = "data: {\"choices\":[]}\n\n";
    let lines = parse_data_lines_without_done(chunk);
    assert_eq!(lines, vec!["{\"choices\":[]}"]);
}

#[test]
fn parse_sse_data_lines_done_filtered() {
    let chunk = "data: [DONE]\n\ndata: {\"choices\":[]}\n\n";
    let lines = parse_data_lines_without_done(chunk);
    assert_eq!(lines, vec!["{\"choices\":[]}"]);
}

#[test]
fn supports_streaming_returns_true() {
    let provider = OpenAiProvider::new(Some("sk-test"));
    assert!(provider.supports_streaming());
}
