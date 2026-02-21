use super::response::{ContentBlock, ProviderMessage, ProviderResponse};
use crate::intelligence::providers::streaming::{
    ProviderChatRequest, ProviderStream, resp_to_events,
};
use crate::intelligence::tools::traits::ToolSpec;
use async_trait::async_trait;
use futures_util::stream;

pub fn messages_to_text(messages: &[ProviderMessage]) -> String {
    messages
        .iter()
        .filter_map(|msg| {
            let role_label = match msg.role {
                super::response::MessageRole::User => "User:",
                super::response::MessageRole::Assistant => "Assistant:",
                super::response::MessageRole::System => "System:",
            };

            let text_parts: Vec<String> = msg
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    ContentBlock::ToolUse { .. }
                    | ContentBlock::ToolResult { .. }
                    | ContentBlock::Image { .. } => None,
                })
                .collect();

            if text_parts.is_empty() {
                None
            } else {
                Some(format!("{} {}", role_label, text_parts.join(" ")))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, message: &str, model: &str, temperature: f64) -> anyhow::Result<String> {
        self.chat_with_system(None, message, model, temperature)
            .await
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String>;

    /// Warm up the HTTP connection pool (TLS handshake, DNS, HTTP/2 setup).
    /// Default implementation is a no-op; providers with HTTP clients should override.
    async fn warmup(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn chat_with_system_full(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let text = self
            .chat_with_system(system_prompt, message, model, temperature)
            .await?;
        Ok(ProviderResponse::text_only(text))
    }

    /// Chat with structured tool support.
    /// Default: concatenates messages into text, ignores tools, falls back to text-only chat.
    async fn chat_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        _tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let text = messages_to_text(messages);
        self.chat_with_system_full(system_prompt, &text, model, temperature)
            .await
    }

    /// Whether this provider supports native structured tool calling.
    fn supports_tool_calling(&self) -> bool {
        false
    }

    /// Whether this provider supports streaming responses.
    fn supports_streaming(&self) -> bool {
        false
    }

    fn supports_vision(&self) -> bool {
        false
    }

    /// Chat with tools and return a stream of events.
    /// Default: converts response to events and returns as a stream.
    async fn chat_with_tools_stream(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        let resp = self
            .chat_with_tools(
                req.system_prompt.as_deref(),
                &req.messages,
                &req.tools,
                &req.model,
                req.temperature,
            )
            .await?;
        Ok(Box::pin(stream::iter(resp_to_events(resp))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intelligence::providers::response::MessageRole;

    #[test]
    fn messages_to_text_concatenates_text_blocks() {
        let messages = vec![
            ProviderMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".to_string(),
                }],
            },
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hi there".to_string(),
                }],
            },
        ];

        let result = messages_to_text(&messages);

        assert_eq!(result, "User: Hello\nAssistant: Hi there");
    }

    #[test]
    fn messages_to_text_skips_tool_use_blocks() {
        let messages = vec![ProviderMessage {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "I'll search".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "search".to_string(),
                    input: serde_json::json!({"q": "rust"}),
                },
            ],
        }];

        let result = messages_to_text(&messages);

        assert_eq!(result, "Assistant: I'll search");
    }

    #[test]
    fn messages_to_text_skips_tool_result_blocks() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: "result".to_string(),
                    is_error: false,
                },
                ContentBlock::Text {
                    text: "Got it".to_string(),
                },
            ],
        }];

        let result = messages_to_text(&messages);

        assert_eq!(result, "User: Got it");
    }

    #[test]
    fn messages_to_text_handles_empty_messages() {
        let messages: Vec<ProviderMessage> = vec![];

        let result = messages_to_text(&messages);

        assert_eq!(result, "");
    }

    #[test]
    fn messages_to_text_skips_messages_with_only_tool_blocks() {
        let messages = vec![ProviderMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"q": "rust"}),
            }],
        }];

        let result = messages_to_text(&messages);

        assert_eq!(result, "");
    }

    #[test]
    fn messages_to_text_handles_multiple_text_blocks_in_one_message() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "Part 1".to_string(),
                },
                ContentBlock::Text {
                    text: "Part 2".to_string(),
                },
            ],
        }];

        let result = messages_to_text(&messages);

        assert_eq!(result, "User: Part 1 Part 2");
    }

    #[test]
    fn messages_to_text_skips_image_blocks() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "Describe this".to_string(),
                },
                ContentBlock::Image {
                    source: crate::intelligence::providers::response::ImageSource::base64(
                        "image/png",
                        "data",
                    ),
                },
            ],
        }];

        let result = messages_to_text(&messages);
        assert_eq!(result, "User: Describe this");
    }

    #[test]
    fn default_supports_tool_calling_returns_false() {
        // Create a minimal mock provider to test the default implementation
        struct MockProvider;

        #[async_trait]
        impl Provider for MockProvider {
            async fn chat_with_system(
                &self,
                _system_prompt: Option<&str>,
                _message: &str,
                _model: &str,
                _temperature: f64,
            ) -> anyhow::Result<String> {
                Ok("response".to_string())
            }
        }

        let provider = MockProvider;
        assert!(!provider.supports_tool_calling());
    }
}
