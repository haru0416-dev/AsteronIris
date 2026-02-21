use crate::core::providers::{
    ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
    build_provider_client, scrub_secret_patterns,
    sse::{SseBuffer, parse_event_data_pairs},
    streaming::{ProviderChatRequest, ProviderStream},
    tool_convert::{ToolFields, map_tools_optional},
    traits::Provider,
};
use crate::core::tools::traits::ToolSpec;
use async_trait::async_trait;
use reqwest::Client;

use super::anthropic_types::{
    AnthropicImageSource, AnthropicToolDef, ChatRequest, ChatResponse, InputContentBlock, Message,
    MessageContent, ResponseContentBlock, StreamContentBlockDelta, StreamContentBlockStart,
    StreamContentBlockType, StreamDelta, StreamMessageDelta, StreamMessageStart,
};

pub struct AnthropicProvider {
    /// Pre-computed auth: `("Authorization", "Bearer <token>")` or `("x-api-key", "<key>")`.
    cached_auth: Option<(&'static str, String)>,
    cached_messages_url: String,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(api_key: Option<&str>) -> Self {
        Self::with_base_url(api_key, None)
    }

    pub fn with_base_url(api_key: Option<&str>, base_url: Option<&str>) -> Self {
        let base = base_url
            .map_or("https://api.anthropic.com", |u| u.trim_end_matches('/'))
            .to_string();
        let cached_messages_url = format!("{base}/v1/messages");
        let cached_auth = api_key.map(str::trim).filter(|k| !k.is_empty()).map(|k| {
            if Self::is_setup_token(k) {
                ("Authorization", format!("Bearer {k}"))
            } else {
                ("x-api-key", k.to_string())
            }
        });
        Self {
            cached_auth,
            cached_messages_url,
            client: build_provider_client(),
        }
    }

    fn is_setup_token(token: &str) -> bool {
        token.starts_with("sk-ant-oat01-")
    }

    fn build_request(
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            max_tokens: 4096,
            system: system_prompt.map(ToString::to_string),
            messages: vec![Message {
                role: "user",
                content: MessageContent::Text(message.to_string()),
            }],
            tools: None,
            temperature,
            stream: None,
        }
    }

    fn provider_message_to_message(provider_message: &ProviderMessage) -> Message {
        let role = match provider_message.role {
            MessageRole::User | MessageRole::System => "user",
            MessageRole::Assistant => "assistant",
        };

        if let [ContentBlock::Text { text }] = provider_message.content.as_slice() {
            return Message {
                role,
                content: MessageContent::Text(scrub_secret_patterns(text).into_owned()),
            };
        }

        let blocks = provider_message
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => InputContentBlock::Text {
                    text: scrub_secret_patterns(text).into_owned(),
                },
                ContentBlock::ToolUse { id, name, input } => InputContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                },
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => InputContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: scrub_secret_patterns(content).into_owned(),
                    is_error: if *is_error { Some(true) } else { None },
                },
                ContentBlock::Image { source } => {
                    let anthropic_source = match source {
                        ImageSource::Base64 { media_type, data } => AnthropicImageSource::Base64 {
                            media_type: media_type.clone(),
                            data: data.clone(),
                        },
                        ImageSource::Url { url } => AnthropicImageSource::Url { url: url.clone() },
                    };
                    InputContentBlock::Image {
                        source: anthropic_source,
                    }
                }
            })
            .collect();

        Message {
            role,
            content: MessageContent::Blocks(blocks),
        }
    }

    fn map_stop_reason(stop_reason: Option<&str>) -> Option<StopReason> {
        stop_reason.map(|reason| match reason {
            "end_turn" => StopReason::EndTurn,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            _ => StopReason::Error,
        })
    }

    fn parse_content_blocks(blocks: &[ResponseContentBlock]) -> Vec<ContentBlock> {
        blocks
            .iter()
            .filter_map(|block| match block {
                ResponseContentBlock::Text { text } => {
                    Some(ContentBlock::Text { text: text.clone() })
                }
                ResponseContentBlock::ToolUse { id, name, input } => Some(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                }),
                ResponseContentBlock::Unsupported => None,
            })
            .collect()
    }

    fn text_from_content_blocks(blocks: &[ContentBlock]) -> Option<String> {
        let text = blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                ContentBlock::ToolUse { .. }
                | ContentBlock::ToolResult { .. }
                | ContentBlock::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        if text.is_empty() { None } else { Some(text) }
    }

    fn extract_text(chat_response: &ChatResponse) -> anyhow::Result<String> {
        Self::text_from_content_blocks(&Self::parse_content_blocks(&chat_response.content))
            .ok_or_else(|| anyhow::anyhow!("No response from Anthropic"))
    }

    async fn call_api(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let request = Self::build_request(system_prompt, message, model, temperature);
        self.call_api_with_request(&request).await
    }

    async fn call_api_with_request(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let (auth_name, auth_value) = self.cached_auth.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Anthropic credentials not set. Set ANTHROPIC_API_KEY or ANTHROPIC_OAUTH_TOKEN (setup-token)."
            )
        })?;

        let response = self
            .client
            .post(&self.cached_messages_url)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header(*auth_name, auth_value)
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("Anthropic", response).await);
        }

        response.json().await.map_err(anyhow::Error::msg)
    }

    async fn call_api_streaming(&self, request: &ChatRequest) -> anyhow::Result<reqwest::Response> {
        let (auth_name, auth_value) = self.cached_auth.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Anthropic credentials not set. Set ANTHROPIC_API_KEY or ANTHROPIC_OAUTH_TOKEN (setup-token)."
            )
        })?;

        let response = self
            .client
            .post(&self.cached_messages_url)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .header(*auth_name, auth_value)
            .json(request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("Anthropic", response).await);
        }

        Ok(response)
    }

    fn parse_sse_events(chunk: &str) -> Vec<(&str, &str)> {
        parse_event_data_pairs(chunk)
    }

    fn stream_events_from_sse(
        event_type: &str,
        data: &str,
        input_tokens: &mut Option<u64>,
        output_tokens: &mut Option<u64>,
    ) -> Vec<crate::core::providers::streaming::StreamEvent> {
        use crate::core::providers::streaming::StreamEvent;

        let mut events = Vec::new();
        match event_type {
            "message_start" => {
                if let Ok(msg) = serde_json::from_str::<StreamMessageStart>(data) {
                    if let Some(usage) = msg.message.usage {
                        *input_tokens = Some(usage.input_tokens);
                    }
                    events.push(StreamEvent::ResponseStart {
                        model: msg.message.model,
                    });
                }
            }
            "content_block_start" => {
                if let Ok(block) = serde_json::from_str::<StreamContentBlockStart>(data) {
                    match block.content_block {
                        StreamContentBlockType::ToolUse { id, name } => {
                            events.push(StreamEvent::ToolCallDelta {
                                index: block.index,
                                id: Some(id),
                                name: Some(name),
                                input_json_delta: String::new(),
                            });
                        }
                        StreamContentBlockType::Text { text } => {
                            if !text.is_empty() {
                                events.push(StreamEvent::TextDelta { text });
                            }
                        }
                        StreamContentBlockType::Unknown => {}
                    }
                }
            }
            "content_block_delta" => {
                if let Ok(delta) = serde_json::from_str::<StreamContentBlockDelta>(data) {
                    match delta.delta {
                        StreamDelta::TextDelta { text } => {
                            events.push(StreamEvent::TextDelta { text });
                        }
                        StreamDelta::InputJsonDelta { partial_json } => {
                            events.push(StreamEvent::ToolCallDelta {
                                index: delta.index,
                                id: None,
                                name: None,
                                input_json_delta: partial_json,
                            });
                        }
                        StreamDelta::Unknown => {}
                    }
                }
            }
            "message_delta" => {
                if let Ok(msg_delta) = serde_json::from_str::<StreamMessageDelta>(data) {
                    if let Some(usage) = msg_delta.usage {
                        *output_tokens = Some(usage.output_tokens);
                    }
                    let stop = Self::map_stop_reason(msg_delta.delta.stop_reason.as_deref());
                    events.push(StreamEvent::Done {
                        stop_reason: stop,
                        input_tokens: *input_tokens,
                        output_tokens: *output_tokens,
                    });
                }
            }
            _ => {}
        }
        events
    }

    async fn chat_with_tools_stream_impl(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        use futures_util::StreamExt;

        let anthropic_messages: Vec<Message> = req
            .messages
            .iter()
            .map(Self::provider_message_to_message)
            .collect();
        let anthropic_tools = map_tools_optional(&req.tools, |tool| {
            let fields = ToolFields::from_tool_with_description(
                tool,
                scrub_secret_patterns(&tool.description).into_owned(),
            );

            AnthropicToolDef {
                name: fields.name,
                description: fields.description,
                input_schema: fields.parameters,
            }
        });

        let request = ChatRequest {
            model: req.model,
            max_tokens: 4096,
            system: req
                .system_prompt
                .map(|system| scrub_secret_patterns(&system).into_owned()),
            messages: anthropic_messages,
            tools: anthropic_tools,
            temperature: req.temperature,
            stream: Some(true),
        };

        let response = self.call_api_streaming(&request).await?;
        let mut byte_stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            let mut sse_buffer = SseBuffer::new();
            let mut input_tokens: Option<u64> = None;
            let mut output_tokens: Option<u64> = None;

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result?;
                sse_buffer.push_chunk(&chunk);

                while let Some(event_block) = sse_buffer.next_event_block() {
                    for (event_type, data) in Self::parse_sse_events(&event_block) {
                        for event in Self::stream_events_from_sse(
                            event_type,
                            data,
                            &mut input_tokens,
                            &mut output_tokens,
                        ) {
                            yield event;
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let chat_response = self
            .call_api(system_prompt, message, model, temperature)
            .await?;
        Self::extract_text(&chat_response)
    }

    async fn chat_with_system_full(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let chat_response = self
            .call_api(system_prompt, message, model, temperature)
            .await?;
        let text = Self::extract_text(&chat_response)?;
        let mut provider_response = if let Some(usage) = chat_response.usage {
            ProviderResponse::with_usage(text, usage.input_tokens, usage.output_tokens)
        } else {
            ProviderResponse::text_only(text)
        };
        if let Some(api_model) = chat_response.model {
            provider_response = provider_response.with_model(api_model);
        }
        Ok(provider_response)
    }

    async fn chat_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let anthropic_messages = messages
            .iter()
            .map(Self::provider_message_to_message)
            .collect();
        let anthropic_tools = map_tools_optional(tools, |tool| {
            let fields = ToolFields::from_tool_with_description(
                tool,
                scrub_secret_patterns(&tool.description).into_owned(),
            );

            AnthropicToolDef {
                name: fields.name,
                description: fields.description,
                input_schema: fields.parameters,
            }
        });

        let request = ChatRequest {
            model: model.to_string(),
            max_tokens: 4096,
            system: system_prompt.map(|system| scrub_secret_patterns(system).into_owned()),
            messages: anthropic_messages,
            tools: anthropic_tools,
            temperature,
            stream: None,
        };
        let chat_response = self.call_api_with_request(&request).await?;

        let content_blocks = Self::parse_content_blocks(&chat_response.content);
        let text = Self::text_from_content_blocks(&content_blocks).unwrap_or_default();

        let mut provider_response = if let Some(usage) = chat_response.usage {
            ProviderResponse::with_usage(text, usage.input_tokens, usage.output_tokens)
        } else {
            ProviderResponse::text_only(text)
        };
        provider_response.content_blocks = content_blocks;
        provider_response.stop_reason = Self::map_stop_reason(chat_response.stop_reason.as_deref());

        if let Some(api_model) = chat_response.model {
            provider_response = provider_response.with_model(api_model);
        }

        Ok(provider_response)
    }

    fn supports_tool_calling(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_vision(&self) -> bool {
        true
    }

    async fn chat_with_tools_stream(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        self.chat_with_tools_stream_impl(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_with_key() {
        let p = AnthropicProvider::new(Some("sk-ant-test123"));
        assert!(p.cached_auth.is_some());
        let (name, value) = p.cached_auth.as_ref().unwrap();
        assert_eq!(*name, "x-api-key");
        assert_eq!(value, "sk-ant-test123");
        assert_eq!(
            p.cached_messages_url,
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn creates_without_key() {
        let p = AnthropicProvider::new(None);
        assert!(p.cached_auth.is_none());
        assert_eq!(
            p.cached_messages_url,
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn creates_with_empty_key() {
        let p = AnthropicProvider::new(Some(""));
        assert!(p.cached_auth.is_none());
    }

    #[test]
    fn creates_with_whitespace_key() {
        let p = AnthropicProvider::new(Some("  sk-ant-test123  "));
        assert!(p.cached_auth.is_some());
        let (name, value) = p.cached_auth.as_ref().unwrap();
        assert_eq!(*name, "x-api-key");
        assert_eq!(value, "sk-ant-test123");
    }

    #[test]
    fn creates_with_custom_base_url() {
        let p =
            AnthropicProvider::with_base_url(Some("sk-ant-test"), Some("https://api.example.com"));
        assert_eq!(p.cached_messages_url, "https://api.example.com/v1/messages");
        let (name, value) = p.cached_auth.as_ref().unwrap();
        assert_eq!(*name, "x-api-key");
        assert_eq!(value, "sk-ant-test");
    }

    #[test]
    fn custom_base_url_trims_trailing_slash() {
        let p = AnthropicProvider::with_base_url(None, Some("https://api.example.com/"));
        assert_eq!(p.cached_messages_url, "https://api.example.com/v1/messages");
    }

    #[test]
    fn default_base_url_when_none_provided() {
        let p = AnthropicProvider::with_base_url(None, None);
        assert_eq!(
            p.cached_messages_url,
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn setup_token_uses_bearer_auth() {
        let p = AnthropicProvider::new(Some("sk-ant-oat01-abc123"));
        let (name, value) = p.cached_auth.as_ref().unwrap();
        assert_eq!(*name, "Authorization");
        assert_eq!(value, "Bearer sk-ant-oat01-abc123");
    }

    #[tokio::test]
    async fn chat_fails_without_key() {
        let p = AnthropicProvider::new(None);
        let result = p
            .chat_with_system(None, "hello", "claude-3-opus", 0.7)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("credentials not set"),
            "Expected key error, got: {err}"
        );
    }

    #[test]
    fn setup_token_detection_works() {
        assert!(AnthropicProvider::is_setup_token("sk-ant-oat01-abcdef"));
        assert!(!AnthropicProvider::is_setup_token("sk-ant-api-key"));
    }

    #[tokio::test]
    async fn chat_with_system_fails_without_key() {
        let p = AnthropicProvider::new(None);
        let result = p
            .chat_with_system(Some("You are AsteronIris"), "hello", "claude-3-opus", 0.7)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn chat_request_serializes_without_system() {
        let req = ChatRequest {
            model: "claude-3-opus".to_string(),
            max_tokens: 4096,
            system: None,
            messages: vec![Message {
                role: "user",
                content: MessageContent::Text("hello".to_string()),
            }],
            tools: None,
            temperature: 0.7,
            stream: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(
            !json.contains("system"),
            "system field should be skipped when None"
        );
        assert!(!json.contains("\"tools\""));
        assert!(json.contains("claude-3-opus"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn chat_request_serializes_with_system() {
        let req = ChatRequest {
            model: "claude-3-opus".to_string(),
            max_tokens: 4096,
            system: Some("You are AsteronIris".to_string()),
            messages: vec![Message {
                role: "user",
                content: MessageContent::Text("hello".to_string()),
            }],
            tools: None,
            temperature: 0.7,
            stream: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"system\":\"You are AsteronIris\""));
    }

    #[test]
    fn chat_request_serializes_with_tools() {
        let req = ChatRequest {
            model: "claude-3-opus".to_string(),
            max_tokens: 4096,
            system: None,
            messages: vec![Message {
                role: "user",
                content: MessageContent::Text("hello".to_string()),
            }],
            tools: Some(vec![AnthropicToolDef {
                name: "shell".to_string(),
                description: "Run a shell command".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    },
                    "required": ["command"]
                }),
            }]),
            temperature: 0.7,
            stream: None,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["tools"][0]["name"], "shell");
        assert_eq!(json["tools"][0]["description"], "Run a shell command");
        assert_eq!(json["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn chat_response_deserializes() {
        let json = r#"{"content":[{"type":"text","text":"Hello there!"}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 1);
        assert!(matches!(
            &resp.content[0],
            ResponseContentBlock::Text { text } if text == "Hello there!"
        ));
    }

    #[test]
    fn chat_response_empty_content() {
        let json = r#"{"content":[]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.content.is_empty());
    }

    #[test]
    fn chat_response_multiple_blocks() {
        let json =
            r#"{"content":[{"type":"text","text":"First"},{"type":"text","text":"Second"}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content.len(), 2);
        assert!(matches!(
            &resp.content[0],
            ResponseContentBlock::Text { text } if text == "First"
        ));
        assert!(matches!(
            &resp.content[1],
            ResponseContentBlock::Text { text } if text == "Second"
        ));
    }

    #[test]
    fn chat_response_tool_use_block_maps_to_provider_content_block() {
        let json = r#"{
            "content":[
                {"type":"tool_use","id":"toolu_1","name":"shell","input":{"command":"ls"}}
            ],
            "stop_reason":"tool_use"
        }"#;

        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        let blocks = AnthropicProvider::parse_content_blocks(&resp.content);

        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            ContentBlock::ToolUse { id, name, input }
            if id == "toolu_1" && name == "shell" && input == &serde_json::json!({"command": "ls"})
        ));
    }

    #[test]
    fn map_stop_reason_handles_tool_use() {
        let reason = AnthropicProvider::map_stop_reason(Some("tool_use"));
        assert_eq!(reason, Some(StopReason::ToolUse));
    }

    #[test]
    fn map_stop_reason_handles_end_turn() {
        let reason = AnthropicProvider::map_stop_reason(Some("end_turn"));
        assert_eq!(reason, Some(StopReason::EndTurn));
    }

    #[test]
    fn provider_message_to_message_supports_tool_result_and_mixed_assistant_blocks() {
        let assistant = ProviderMessage {
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "Running shell".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "shell".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ],
        };
        let tool_result = ProviderMessage::tool_result("toolu_1", "src", false);

        let assistant_message = AnthropicProvider::provider_message_to_message(&assistant);
        let tool_result_message = AnthropicProvider::provider_message_to_message(&tool_result);

        let assistant_json = serde_json::to_value(&assistant_message).unwrap();
        let tool_result_json = serde_json::to_value(&tool_result_message).unwrap();

        assert_eq!(assistant_json["role"], "assistant");
        assert_eq!(assistant_json["content"][0]["type"], "text");
        assert_eq!(assistant_json["content"][1]["type"], "tool_use");
        assert_eq!(tool_result_json["role"], "user");
        assert_eq!(tool_result_json["content"][0]["type"], "tool_result");
        assert_eq!(tool_result_json["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(tool_result_json["content"][0]["content"], "src");
    }

    #[test]
    fn provider_message_to_message_maps_image_block() {
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
        let mapped = AnthropicProvider::provider_message_to_message(&msg);
        assert_eq!(mapped.role, "user");

        let json = serde_json::to_value(&mapped.content).unwrap();
        let blocks = json.as_array().expect("content should be array");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["type"], "base64");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
    }

    #[test]
    fn supports_tool_calling_returns_true() {
        let provider = AnthropicProvider::new(Some("sk-ant-test123"));
        assert!(provider.supports_tool_calling());
    }

    #[test]
    fn supports_streaming_returns_true() {
        let provider = AnthropicProvider::new(Some("sk-ant-test123"));
        assert!(provider.supports_streaming());
    }

    #[test]
    fn supports_vision_returns_true() {
        let provider = AnthropicProvider::new(Some("test-key"));
        assert!(provider.supports_vision());
    }

    #[test]
    fn parse_sse_events_basic() {
        let chunk = "event: message_start\ndata: {\"message\":{}}\n\n";
        let events = AnthropicProvider::parse_sse_events(chunk);
        assert_eq!(events, vec![("message_start", "{\"message\":{}}")]);
    }

    #[test]
    fn parse_sse_events_multiple() {
        let chunk = concat!(
            "event: message_start\n",
            "data: {\"message\":{}}\n\n",
            "event: content_block_delta\n",
            "data: {\"delta\":{}}\n\n"
        );
        let events = AnthropicProvider::parse_sse_events(chunk);
        assert_eq!(
            events,
            vec![
                ("message_start", "{\"message\":{}}"),
                ("content_block_delta", "{\"delta\":{}}")
            ]
        );
    }

    #[test]
    fn parse_sse_events_empty() {
        let events = AnthropicProvider::parse_sse_events("");
        assert!(events.is_empty());
    }

    #[test]
    fn temperature_range_serializes() {
        for temp in [0.0, 0.5, 1.0, 2.0] {
            let req = ChatRequest {
                model: "claude-3-opus".to_string(),
                max_tokens: 4096,
                system: None,
                messages: vec![],
                tools: None,
                temperature: temp,
                stream: None,
            };
            let json = serde_json::to_string(&req).unwrap();
            assert!(json.contains(&format!("{temp}")));
        }
    }
}
