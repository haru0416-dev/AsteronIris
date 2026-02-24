use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

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
    Image {
        source: ImageSource,
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

    pub fn user_with_image(text: impl Into<String>, source: ImageSource) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text { text: text.into() },
                ContentBlock::Image { source },
            ],
        }
    }

    pub fn user_image(source: ImageSource) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![ContentBlock::Image { source }],
        }
    }
}

impl ImageSource {
    pub fn base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self::Base64 {
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    pub fn url(url: impl Into<String>) -> Self {
        Self::Url { url: url.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
    };

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
    fn provider_response_has_tool_use_works() {
        let with = ProviderResponse {
            text: "done".into(),
            input_tokens: None,
            output_tokens: None,
            model: None,
            content_blocks: vec![ContentBlock::ToolUse {
                id: "toolu_1".into(),
                name: "search".into(),
                input: serde_json::json!({"q": "rust"}),
            }],
            stop_reason: Some(StopReason::ToolUse),
        };
        let without = ProviderResponse::text_only("done".into());
        assert!(with.has_tool_use());
        assert!(!without.has_tool_use());
    }

    #[test]
    fn text_only_and_with_usage() {
        let text_only = ProviderResponse::text_only("hello".into());
        assert_eq!(text_only.total_tokens(), None);
        let with_usage = ProviderResponse::with_usage("hello".into(), 10, 20);
        assert_eq!(with_usage.total_tokens(), Some(30));
    }

    #[test]
    fn image_source_constructors() {
        let b64 = ImageSource::base64("image/png", "data");
        assert!(matches!(b64, ImageSource::Base64 { .. }));
        let url = ImageSource::url("https://example.com/img.png");
        assert!(matches!(url, ImageSource::Url { .. }));
    }
}
