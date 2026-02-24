use super::types::{ContentBlock, MessageRole, ProviderMessage, ProviderResponse};
use crate::tools::ToolSpec;
use futures_util::stream;
use std::future::Future;
use std::pin::Pin;

pub fn messages_to_text(messages: &[ProviderMessage]) -> String {
    messages
        .iter()
        .filter_map(|msg| {
            let role_label = match msg.role {
                MessageRole::User => "User:",
                MessageRole::Assistant => "Assistant:",
                MessageRole::System => "System:",
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

use super::streaming::{ProviderStream, resp_to_events};

/// Provider capabilities reported at runtime.
#[derive(Debug, Clone, Default)]
pub struct ProviderCapabilities {
    pub tool_calling: bool,
    pub streaming: bool,
    pub vision: bool,
}

pub trait Provider: Send + Sync {
    /// Provider identifier (e.g. "anthropic", "openai").
    fn name(&self) -> &str;

    /// Runtime capability flags.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    fn chat<'a>(
        &'a self,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            self.chat_with_system(None, message, model, temperature)
                .await
        })
    }

    fn chat_with_system<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>>;

    /// Warm up the HTTP connection pool.
    fn warmup(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async move { Ok(()) })
    }

    fn chat_with_system_full<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let text = self
                .chat_with_system(system_prompt, message, model, temperature)
                .await?;
            Ok(ProviderResponse::text_only(text))
        })
    }

    fn chat_with_tools<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        messages: &'a [ProviderMessage],
        _tools: &'a [ToolSpec],
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let text = messages_to_text(messages);
            self.chat_with_system_full(system_prompt, &text, model, temperature)
                .await
        })
    }

    fn supports_tool_calling(&self) -> bool {
        self.capabilities().tool_calling
    }

    fn supports_streaming(&self) -> bool {
        self.capabilities().streaming
    }

    fn supports_vision(&self) -> bool {
        self.capabilities().vision
    }

    fn chat_with_tools_stream<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        messages: &'a [ProviderMessage],
        tools: &'a [ToolSpec],
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send + 'a>> {
        Box::pin(async move {
            let resp = self
                .chat_with_tools(system_prompt, messages, tools, model, temperature)
                .await?;
            Ok(Box::pin(stream::iter(resp_to_events(resp))) as ProviderStream)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_to_text_concatenates_text_blocks() {
        let messages = vec![
            ProviderMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hello".into(),
                }],
            },
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hi there".into(),
                }],
            },
        ];
        assert_eq!(
            messages_to_text(&messages),
            "User: Hello\nAssistant: Hi there"
        );
    }

    #[test]
    fn messages_to_text_skips_tool_blocks() {
        let messages = vec![ProviderMessage {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "I'll search".into(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "search".into(),
                    input: serde_json::json!({"q": "rust"}),
                },
            ],
        }];
        assert_eq!(messages_to_text(&messages), "Assistant: I'll search");
    }

    #[test]
    fn messages_to_text_handles_empty() {
        assert_eq!(messages_to_text(&[]), "");
    }

    #[test]
    fn default_capabilities_are_all_false() {
        let caps = ProviderCapabilities::default();
        assert!(!caps.tool_calling);
        assert!(!caps.streaming);
        assert!(!caps.vision);
    }
}
