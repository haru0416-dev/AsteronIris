use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMessage {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub model: Option<String>,
    pub content_blocks: Vec<ContentBlock>,
    pub stop_reason: Option<StopReason>,
}

impl ProviderResponse {
    pub fn text_only(text: String) -> Self {
        Self {
            text,
            input_tokens: None,
            output_tokens: None,
            model: None,
            content_blocks: vec![],
            stop_reason: None,
        }
    }

    pub fn with_usage(text: String, input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            text,
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            model: None,
            content_blocks: vec![],
            stop_reason: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn total_tokens(&self) -> Option<u64> {
        match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        }
    }

    pub fn tool_use_blocks(&self) -> Vec<&ContentBlock> {
        self.content_blocks
            .iter()
            .filter(|block| matches!(block, ContentBlock::ToolUse { .. }))
            .collect()
    }

    pub fn has_tool_use(&self) -> bool {
        self.content_blocks
            .iter()
            .any(|block| matches!(block, ContentBlock::ToolUse { .. }))
    }

    pub fn to_assistant_message(&self) -> ProviderMessage {
        if self.content_blocks.is_empty() {
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: self.text.clone(),
                }],
            }
        } else {
            ProviderMessage {
                role: MessageRole::Assistant,
                content: self.content_blocks.clone(),
            }
        }
    }
}

impl ProviderMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ContentBlock, MessageRole, ProviderMessage, ProviderResponse, StopReason};

    #[test]
    fn content_block_serde_round_trip() {
        let value = serde_json::json!({
            "type": "tool_use",
            "id": "toolu_123",
            "name": "search",
            "input": {"query": "rust"}
        });

        let block: ContentBlock = serde_json::from_value(value.clone()).unwrap();
        let serialized = serde_json::to_value(&block).unwrap();

        assert_eq!(serialized, value);
    }

    #[test]
    fn provider_message_user_constructor() {
        let message = ProviderMessage::user("hello");

        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("expected text content block"),
        }
    }

    #[test]
    fn provider_message_tool_result_constructor() {
        let message = ProviderMessage::tool_result("toolu_123", "ok", false);

        assert_eq!(message.role, MessageRole::User);
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "toolu_123");
                assert_eq!(content, "ok");
                assert!(!is_error);
            }
            _ => panic!("expected tool_result content block"),
        }
    }

    #[test]
    fn provider_response_tool_use_blocks_filters_correctly() {
        let response = ProviderResponse {
            text: "done".to_string(),
            input_tokens: None,
            output_tokens: None,
            model: None,
            content_blocks: vec![
                ContentBlock::Text {
                    text: "hi".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "search".to_string(),
                    input: serde_json::json!({"q": "rust"}),
                },
                ContentBlock::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: "result".to_string(),
                    is_error: false,
                },
                ContentBlock::ToolUse {
                    id: "toolu_2".to_string(),
                    name: "read_file".to_string(),
                    input: serde_json::json!({"path": "src/lib.rs"}),
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
        };

        let blocks = response.tool_use_blocks();

        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], ContentBlock::ToolUse { .. }));
        assert!(matches!(blocks[1], ContentBlock::ToolUse { .. }));
    }

    #[test]
    fn provider_response_has_tool_use_works() {
        let with_tool_use = ProviderResponse {
            text: "done".to_string(),
            input_tokens: None,
            output_tokens: None,
            model: None,
            content_blocks: vec![ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"q": "rust"}),
            }],
            stop_reason: Some(StopReason::ToolUse),
        };
        let without_tool_use = ProviderResponse {
            text: "done".to_string(),
            input_tokens: None,
            output_tokens: None,
            model: None,
            content_blocks: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            stop_reason: Some(StopReason::EndTurn),
        };

        assert!(with_tool_use.has_tool_use());
        assert!(!without_tool_use.has_tool_use());
    }

    #[test]
    fn provider_response_to_assistant_message_empty_content_blocks() {
        let response = ProviderResponse::text_only("plain text".to_string());

        let message = response.to_assistant_message();

        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.content.len(), 1);
        match &message.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "plain text"),
            _ => panic!("expected text content block"),
        }
    }

    #[test]
    fn provider_response_to_assistant_message_non_empty_content_blocks() {
        let response = ProviderResponse {
            text: "fallback".to_string(),
            input_tokens: None,
            output_tokens: None,
            model: None,
            content_blocks: vec![ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"q": "rust"}),
            }],
            stop_reason: Some(StopReason::ToolUse),
        };

        let message = response.to_assistant_message();

        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.content.len(), 1);
        assert!(matches!(message.content[0], ContentBlock::ToolUse { .. }));
    }

    #[test]
    fn stop_reason_serde_round_trip() {
        let reason = StopReason::MaxTokens;

        let value = serde_json::to_value(reason).unwrap();
        assert_eq!(value, serde_json::json!("max_tokens"));

        let decoded: StopReason = serde_json::from_value(value).unwrap();
        assert_eq!(decoded, StopReason::MaxTokens);
    }

    #[test]
    fn text_only_and_with_usage_still_work() {
        let text_only = ProviderResponse::text_only("hello".to_string());
        assert_eq!(text_only.text, "hello");
        assert_eq!(text_only.input_tokens, None);
        assert_eq!(text_only.output_tokens, None);
        assert_eq!(text_only.model, None);
        assert!(text_only.content_blocks.is_empty());
        assert_eq!(text_only.stop_reason, None);
        assert_eq!(text_only.total_tokens(), None);

        let with_usage = ProviderResponse::with_usage("hello".to_string(), 10, 20);
        assert_eq!(with_usage.text, "hello");
        assert_eq!(with_usage.input_tokens, Some(10));
        assert_eq!(with_usage.output_tokens, Some(20));
        assert_eq!(with_usage.model, None);
        assert!(with_usage.content_blocks.is_empty());
        assert_eq!(with_usage.stop_reason, None);
        assert_eq!(with_usage.total_tokens(), Some(30));
    }
}
