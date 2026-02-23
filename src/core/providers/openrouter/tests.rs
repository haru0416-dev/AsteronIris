use super::*;
use crate::core::providers::{ContentBlock, ImageSource, MessageRole, Provider};

#[test]
fn tools_request_serializes_in_openai_function_format() {
    let messages = vec![ProviderMessage::user("list files")];
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

    let request =
        OpenRouterProvider::build_tools_request(None, &messages, &tools, "gpt-4o-mini", 0.3);
    let json = serde_json::to_value(&request).unwrap();

    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tools"][0]["function"]["name"], "shell");
    assert_eq!(json["tools"][0]["function"]["parameters"]["type"], "object");
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
    let messages = OpenRouterProvider::map_provider_message(&msg);
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
    let provider = OpenRouterProvider::new(Some("or-key"));
    assert!(provider.supports_tool_calling());
}

#[test]
fn supports_vision_returns_true() {
    let provider = OpenRouterProvider::new(Some("test-key"));
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
    let provider = OpenRouterProvider::new(Some("or-key"));
    assert!(provider.supports_streaming());
}
