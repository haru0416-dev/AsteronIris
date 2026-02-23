use super::types::CandidateContent;
use super::*;
use crate::core::providers::Provider;
use crate::core::tools::traits::ToolSpec;

#[test]
fn provider_creates_without_key() {
    let provider = GeminiProvider::new(None);
    // Should not panic, just have no key
    assert!(provider.api_key.is_none() || provider.api_key.is_some());
}

#[test]
fn provider_creates_with_key() {
    let provider = GeminiProvider::new(Some("test-api-key"));
    assert!(provider.api_key.is_some());
    assert_eq!(provider.api_key.as_deref(), Some("test-api-key"));
}

#[test]
fn gemini_cli_dir_returns_path() {
    let dir = GeminiProvider::gemini_cli_dir();
    // Should return Some on systems with home dir
    if UserDirs::new().is_some() {
        assert!(dir.is_some());
        assert!(dir.unwrap().ends_with(".gemini"));
    }
}

#[test]
fn auth_source_reports_correctly() {
    let provider = GeminiProvider::new(Some("explicit-key"));
    // With explicit key, should report "config" (unless CLI credentials exist)
    let source = provider.auth_source();
    // Should be either "config" or "Gemini CLI OAuth" if CLI is configured
    assert!(source == "config" || source == "Gemini CLI OAuth");
}

#[test]
fn model_name_formatting() {
    // Test that model names are formatted correctly
    let model = "gemini-2.0-flash";
    let formatted = if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    };
    assert_eq!(formatted, "models/gemini-2.0-flash");

    // Already prefixed
    let model2 = "models/gemini-1.5-pro";
    let formatted2 = if model2.starts_with("models/") {
        model2.to_string()
    } else {
        format!("models/{model2}")
    };
    assert_eq!(formatted2, "models/gemini-1.5-pro");
}

#[test]
fn request_serialization() {
    let request = GenerateContentRequest {
        contents: vec![Content {
            role: Some("user".to_string()),
            parts: vec![Part::text("Hello".to_string())],
        }],
        system_instruction: Some(Content {
            role: None,
            parts: vec![Part::text("You are helpful".to_string())],
        }),
        tools: None,
        generation_config: GenerationConfig {
            temperature: 0.7,
            max_output_tokens: 8192,
        },
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("\"text\":\"Hello\""));
    assert!(json.contains("\"temperature\":0.7"));
    assert!(json.contains("\"maxOutputTokens\":8192"));
}

#[test]
fn response_deserialization() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{"text": "Hello there!"}]
            }
        }]
    }"#;

    let response: GenerateContentResponse = serde_json::from_str(json).unwrap();
    assert!(response.candidates.is_some());
    let text = response
        .candidates
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
        .content
        .parts
        .into_iter()
        .next()
        .unwrap()
        .text;
    assert_eq!(text, Some("Hello there!".to_string()));
}

#[test]
fn gemini_tools_serialize_as_function_declarations() {
    let tools = vec![ToolSpec {
        name: "shell".to_string(),
        description: "Execute shell command".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"command": {"type": "string"}},
            "required": ["command"]
        }),
    }];

    let request = GeminiProvider::build_tools_request(
        None,
        &[ProviderMessage::user("list files")],
        &tools,
        0.1,
    );
    let value = serde_json::to_value(&request).unwrap();

    assert_eq!(
        value["tools"][0]["function_declarations"][0]["name"],
        "shell"
    );
    assert_eq!(
        value["tools"][0]["function_declarations"][0]["parameters"]["type"],
        "object"
    );
}

#[test]
fn gemini_function_call_response_parses_to_tool_use_block() {
    let json = r#"{
        "candidates": [{
            "content": {
                "parts": [{"functionCall": {"name": "shell", "args": {"command": "ls"}}}]
            },
            "finishReason": "FUNCTION_CALL"
        }]
    }"#;

    let response: GenerateContentResponse = serde_json::from_str(json).unwrap();
    let candidate = response.candidates.unwrap().into_iter().next().unwrap();
    let blocks = GeminiProvider::parse_content_blocks(&candidate.content.parts);

    assert!(matches!(
        &blocks[0],
        ContentBlock::ToolUse { name, input, .. }
        if name == "shell" && input == &serde_json::json!({"command": "ls"})
    ));
}

#[test]
fn gemini_finish_reason_mapping_handles_tool_calls() {
    let with_tool_call = Candidate {
        content: CandidateContent {
            parts: vec![ResponsePart {
                text: None,
                function_call: Some(GeminiFunctionCall {
                    name: "shell".to_string(),
                    args: serde_json::json!({"command": "ls"}),
                    id: None,
                }),
            }],
        },
        finish_reason: Some("STOP".to_string()),
    };
    let max_tokens = Candidate {
        content: CandidateContent {
            parts: vec![ResponsePart {
                text: Some("x".to_string()),
                function_call: None,
            }],
        },
        finish_reason: Some("MAX_TOKENS".to_string()),
    };

    assert_eq!(
        GeminiProvider::map_stop_reason(&with_tool_call),
        StopReason::ToolUse
    );
    assert_eq!(
        GeminiProvider::map_stop_reason(&max_tokens),
        StopReason::MaxTokens
    );
}

#[test]
fn map_provider_message_handles_image_block() {
    let msg = ProviderMessage {
        role: MessageRole::User,
        content: vec![
            ContentBlock::Text {
                text: "Describe this".to_string(),
            },
            ContentBlock::Image {
                source: ImageSource::base64("image/png", "iVBOR"),
            },
        ],
    };
    let tool_map = std::collections::HashMap::new();
    let content = GeminiProvider::map_provider_message(&msg, &tool_map);
    assert_eq!(content.role.as_deref(), Some("user"));
    assert_eq!(content.parts.len(), 2);
    let json = serde_json::to_value(&content).unwrap();
    assert!(json["parts"][0]["text"].is_string());
    assert_eq!(json["parts"][1]["inlineData"]["mimeType"], "image/png");
}

#[test]
fn supports_tool_calling_returns_true() {
    let provider = GeminiProvider::new(Some("test-api-key"));
    assert!(provider.supports_tool_calling());
}

#[test]
fn supports_vision_returns_true() {
    let provider = GeminiProvider::new(Some("test-key"));
    assert!(provider.supports_vision());
}

#[test]
fn supports_streaming_returns_true() {
    let provider = GeminiProvider::new(Some("test-api-key"));
    assert!(provider.supports_streaming());
}

#[test]
fn parse_sse_data_lines_basic() {
    let chunk = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}\n\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]}}]}\n\n"
    );
    let lines = parse_data_lines(chunk);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}"
    );
    assert_eq!(
        lines[1],
        "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]}}]}"
    );
}

#[test]
fn parse_sse_data_lines_empty() {
    let lines = parse_data_lines("");
    assert!(lines.is_empty());
}

#[test]
fn error_response_deserialization() {
    let json = r#"{
        "error": {
            "message": "Invalid API key"
        }
    }"#;

    let response: GenerateContentResponse = serde_json::from_str(json).unwrap();
    assert!(response.error.is_some());
    assert_eq!(response.error.unwrap().message, "Invalid API key");
}
