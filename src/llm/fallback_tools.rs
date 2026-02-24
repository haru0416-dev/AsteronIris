use super::types::{ContentBlock, ProviderResponse, StopReason};
use crate::tools::ToolSpec;
use serde_json::Value;
use std::collections::HashSet;
use std::fmt::Write;

const TOOL_CALL_OPEN_TAG: &str = "<tool_call>";
const TOOL_CALL_CLOSE_TAG: &str = "</tool_call>";

#[derive(Debug, Clone)]
pub struct ExtractedToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[must_use]
pub fn augment_system_prompt_with_tools(system_prompt: &str, tools: &[ToolSpec]) -> String {
    let mut augmented = String::from(system_prompt);
    augmented.push_str("\n\n## Available Tools\n\n");
    augmented.push_str("To use a tool, respond with exactly this format:\n");
    augmented.push_str("<tool_call>\n");
    augmented.push_str("{\"name\": \"tool_name\", \"arguments\": {...}}\n");
    augmented.push_str("</tool_call>\n\n");
    augmented.push_str(
        "You may use multiple tool calls in a single response. After receiving tool results, continue your response.\n\n",
    );
    augmented.push_str("Available tools:\n");

    if tools.is_empty() {
        augmented.push_str("- (none)\n");
        return augmented;
    }

    for tool in tools {
        let parameters = match serde_json::to_string(&tool.parameters) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    tool = tool.name,
                    "Failed to serialize tool parameters: {error}"
                );
                "{}".to_string()
            }
        };

        let _ = writeln!(
            augmented,
            "- {}: {} Parameters: {}",
            tool.name, tool.description, parameters
        );
    }

    augmented
}

#[must_use]
pub fn extract_tool_calls(
    response_text: &str,
    valid_tools: &[ToolSpec],
) -> (String, Vec<ExtractedToolCall>) {
    let valid_names: HashSet<&str> = valid_tools.iter().map(|tool| tool.name.as_str()).collect();

    let mut remaining_text = String::with_capacity(response_text.len());
    let mut extracted_calls = Vec::new();
    let mut search_start = 0;
    let mut call_counter = 1;

    while let Some(open_offset) = response_text[search_start..].find(TOOL_CALL_OPEN_TAG) {
        let open_index = search_start + open_offset;
        remaining_text.push_str(&response_text[search_start..open_index]);

        let content_start = open_index + TOOL_CALL_OPEN_TAG.len();
        if let Some(close_offset) = response_text[content_start..].find(TOOL_CALL_CLOSE_TAG) {
            let close_index = content_start + close_offset;
            let block_content = &response_text[content_start..close_index];

            if let Some(call) = parse_tool_call(block_content, &valid_names, call_counter) {
                extracted_calls.push(call);
                call_counter += 1;
            } else {
                let block_end = close_index + TOOL_CALL_CLOSE_TAG.len();
                remaining_text.push_str(&response_text[open_index..block_end]);
            }

            search_start = close_index + TOOL_CALL_CLOSE_TAG.len();
        } else {
            remaining_text.push_str(&response_text[open_index..]);
            search_start = response_text.len();
            break;
        }
    }

    if search_start < response_text.len() {
        remaining_text.push_str(&response_text[search_start..]);
    }

    (remaining_text, extracted_calls)
}

fn parse_tool_call(
    block_content: &str,
    valid_names: &HashSet<&str>,
    call_counter: usize,
) -> Option<ExtractedToolCall> {
    let parsed_json = match serde_json::from_str::<Value>(block_content.trim()) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!("Discarding malformed fallback tool call JSON: {error}");
            return None;
        }
    };

    let Some(name) = parsed_json.get("name").and_then(Value::as_str) else {
        tracing::warn!("Discarding fallback tool call without a string name field");
        return None;
    };

    if !valid_names.contains(name) {
        tracing::warn!(
            tool = name,
            "Discarding fallback tool call for unknown tool"
        );
        return None;
    }

    let Some(arguments) = parsed_json.get("arguments") else {
        tracing::warn!(
            tool = name,
            "Discarding fallback tool call without arguments field"
        );
        return None;
    };

    Some(ExtractedToolCall {
        id: format!("fallback_call_{call_counter}"),
        name: name.to_string(),
        input: arguments.clone(),
    })
}

#[must_use]
pub fn build_fallback_response(
    mut response: ProviderResponse,
    valid_tools: &[ToolSpec],
) -> ProviderResponse {
    let (remaining_text, tool_calls) = extract_tool_calls(&response.text, valid_tools);

    if tool_calls.is_empty() {
        response.stop_reason = Some(StopReason::EndTurn);
        return response;
    }

    let mut blocks = Vec::new();
    if !remaining_text.trim().is_empty() {
        blocks.push(ContentBlock::Text {
            text: remaining_text.clone(),
        });
    }

    for call in tool_calls {
        blocks.push(ContentBlock::ToolUse {
            id: call.id,
            name: call.name,
            input: call.input,
        });
    }

    response.text = remaining_text;
    response.content_blocks = blocks;
    response.stop_reason = Some(StopReason::ToolUse);
    response
}

#[cfg(test)]
mod tests {
    use super::{
        ExtractedToolCall, augment_system_prompt_with_tools, build_fallback_response,
        extract_tool_calls,
    };
    use crate::llm::types::{ContentBlock, ProviderResponse, StopReason};
    use crate::tools::ToolSpec;
    use serde_json::json;

    fn sample_tools() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "shell".to_string(),
                description: "Execute a shell command.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {"command": {"type": "string"}},
                    "required": ["command"]
                }),
            },
            ToolSpec {
                name: "file_read".to_string(),
                description: "Read file contents.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }),
            },
        ]
    }

    #[test]
    fn extract_tool_calls_parses_valid_tool_call() {
        let tools = sample_tools();
        let text =
            "<tool_call>{\"name\": \"shell\", \"arguments\": {\"command\": \"ls\"}}</tool_call>";

        let (remaining, calls) = extract_tool_calls(text, &tools);

        assert_eq!(remaining, "");
        assert_eq!(calls.len(), 1);
        assert_tool_call(
            &calls[0],
            "fallback_call_1",
            "shell",
            json!({"command": "ls"}),
        );
    }

    #[test]
    fn extract_tool_calls_discards_unknown_tool_names() {
        let tools = sample_tools();
        let text = "<tool_call>{\"name\": \"fake_tool\", \"arguments\": {}}</tool_call>";

        let (remaining, calls) = extract_tool_calls(text, &tools);

        assert!(calls.is_empty());
        assert_eq!(remaining, text);
    }

    #[test]
    fn extract_tool_calls_preserves_text_without_tool_blocks() {
        let tools = sample_tools();
        let text = "Just a normal response.";

        let (remaining, calls) = extract_tool_calls(text, &tools);

        assert!(calls.is_empty());
        assert_eq!(remaining, text);
    }

    #[test]
    fn extract_tool_calls_discards_malformed_json_and_keeps_original_text() {
        let tools = sample_tools();
        let text = "<tool_call>{\"name\": \"shell\", \"arguments\": {\"command\": }}</tool_call>";

        let (remaining, calls) = extract_tool_calls(text, &tools);

        assert!(calls.is_empty());
        assert_eq!(remaining, text);
    }

    #[test]
    fn extract_tool_calls_parses_multiple_tool_calls() {
        let tools = sample_tools();
        let text = concat!(
            "<tool_call>{\"name\": \"shell\", \"arguments\": {\"command\": \"pwd\"}}</tool_call>",
            "\n",
            "<tool_call>{\"name\": \"file_read\", \"arguments\": {\"path\": \"src/lib.rs\"}}</tool_call>"
        );

        let (remaining, calls) = extract_tool_calls(text, &tools);

        assert_eq!(remaining, "\n");
        assert_eq!(calls.len(), 2);
        assert_tool_call(
            &calls[0],
            "fallback_call_1",
            "shell",
            json!({"command": "pwd"}),
        );
        assert_tool_call(
            &calls[1],
            "fallback_call_2",
            "file_read",
            json!({"path": "src/lib.rs"}),
        );
    }

    #[test]
    fn augment_system_prompt_with_tools_includes_names_and_schema() {
        let tools = sample_tools();
        let prompt = augment_system_prompt_with_tools("System prompt", &tools);

        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("<tool_call>"));
        assert!(prompt.contains("shell: Execute a shell command."));
        assert!(prompt.contains("file_read: Read file contents."));
        assert!(prompt.contains("\"required\":[\"command\"]"));
    }

    #[test]
    fn build_fallback_response_sets_stop_reason_when_tool_calls_exist() {
        let tools = sample_tools();
        let response = ProviderResponse::text_only(
            "<tool_call>{\"name\": \"shell\", \"arguments\": {\"command\": \"ls\"}}</tool_call>"
                .to_string(),
        );

        let built = build_fallback_response(response, &tools);

        assert_eq!(built.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(built.text, "");
        assert_eq!(built.content_blocks.len(), 1);
        assert!(matches!(
            built.content_blocks[0],
            ContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn build_fallback_response_keeps_end_turn_when_no_tool_calls() {
        let tools = sample_tools();
        let response = ProviderResponse::text_only("No tools requested".to_string());

        let built = build_fallback_response(response, &tools);

        assert_eq!(built.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(built.text, "No tools requested");
        assert!(built.content_blocks.is_empty());
    }

    #[test]
    fn build_fallback_response_preserves_text_and_extracts_tool_calls() {
        let tools = sample_tools();
        let response = ProviderResponse::text_only(
            concat!(
                "I will inspect files.\n",
                "<tool_call>{\"name\": \"shell\", \"arguments\": {\"command\": \"ls\"}}</tool_call>",
                "\nThen I will continue."
            )
            .to_string(),
        );

        let built = build_fallback_response(response, &tools);

        assert_eq!(built.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(built.text, "I will inspect files.\n\nThen I will continue.");
        assert_eq!(built.content_blocks.len(), 2);
        assert!(matches!(built.content_blocks[0], ContentBlock::Text { .. }));
        assert!(matches!(
            built.content_blocks[1],
            ContentBlock::ToolUse { .. }
        ));
    }

    fn assert_tool_call(call: &ExtractedToolCall, id: &str, name: &str, input: serde_json::Value) {
        assert_eq!(call.id, id);
        assert_eq!(call.name, name);
        assert_eq!(call.input, input);
    }
}
