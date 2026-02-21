use super::openai_types::{
    ChatCompletionChunk, ChatRequest, ChatResponse, ContentPart, ImageUrlContent, Message,
    MessageContent, OpenAiTool, OpenAiToolCall, OpenAiToolCallFunction, OpenAiToolDefinition,
    StreamOptions,
};
use crate::core::providers::{
    ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
    build_provider_client, scrub_secret_patterns,
    sse::{SseBuffer, parse_data_lines_without_done},
    streaming::{ProviderChatRequest, ProviderStream},
    tool_convert::{ToolFields, map_tools_optional},
    traits::Provider,
};
use crate::core::tools::traits::ToolSpec;
use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

pub struct OpenAiProvider {
    /// Pre-computed `"Bearer <key>"` header value (avoids `format!` per request).
    cached_auth_header: Option<String>,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<&str>) -> Self {
        Self {
            cached_auth_header: api_key.map(|k| format!("Bearer {k}")),
            client: build_provider_client(),
        }
    }

    fn build_request(
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> ChatRequest {
        let capacity = if system_prompt.is_some() { 2 } else { 1 };
        let mut messages = Vec::with_capacity(capacity);

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system",
                content: Some(MessageContent::Text(sys.to_string())),
                tool_call_id: None,
                tool_calls: None,
            });
        }

        messages.push(Message {
            role: "user",
            content: Some(MessageContent::Text(message.to_string())),
            tool_call_id: None,
            tool_calls: None,
        });

        ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            tools: None,
            stream: None,
            stream_options: None,
        }
    }

    fn build_text_message(role: &'static str, content: String) -> Message {
        Message {
            role,
            content: Some(MessageContent::Text(content)),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    fn map_provider_message(provider_message: &ProviderMessage) -> Vec<Message> {
        let mut text_parts = Vec::new();
        let mut image_parts = Vec::new();
        let mut assistant_tool_calls = Vec::new();
        let mut tool_messages = Vec::new();

        for block in &provider_message.content {
            match block {
                ContentBlock::Text { text } => {
                    text_parts.push(scrub_secret_patterns(text).into_owned());
                }
                ContentBlock::ToolUse { id, name, input } => {
                    assistant_tool_calls.push(OpenAiToolCall {
                        id: id.clone(),
                        r#type: "function".to_string(),
                        function: OpenAiToolCallFunction {
                            name: name.clone(),
                            arguments: input.to_string(),
                        },
                    });
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error: _,
                } => {
                    tool_messages.push(Message {
                        role: "tool",
                        content: Some(MessageContent::Text(
                            scrub_secret_patterns(content).into_owned(),
                        )),
                        tool_call_id: Some(tool_use_id.clone()),
                        tool_calls: None,
                    });
                }
                ContentBlock::Image { source } => {
                    let url = match source {
                        ImageSource::Base64 { media_type, data } => {
                            format!("data:{media_type};base64,{data}")
                        }
                        ImageSource::Url { url } => url.clone(),
                    };
                    image_parts.push(ContentPart::ImageUrl {
                        image_url: ImageUrlContent { url },
                    });
                }
            }
        }

        let mut messages = Vec::new();
        let text_content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join("\n"))
        };

        match provider_message.role {
            MessageRole::Assistant => {
                if text_content.is_some() || !assistant_tool_calls.is_empty() {
                    messages.push(Message {
                        role: "assistant",
                        content: text_content.map(MessageContent::Text),
                        tool_call_id: None,
                        tool_calls: if assistant_tool_calls.is_empty() {
                            None
                        } else {
                            Some(assistant_tool_calls)
                        },
                    });
                }
            }
            MessageRole::User => {
                if image_parts.is_empty() {
                    if let Some(content) = text_content {
                        messages.push(Self::build_text_message("user", content));
                    }
                } else {
                    let mut parts = Vec::new();
                    if let Some(text) = text_content {
                        parts.push(ContentPart::Text { text });
                    }
                    parts.extend(image_parts);
                    messages.push(Message {
                        role: "user",
                        content: Some(MessageContent::Parts(parts)),
                        tool_call_id: None,
                        tool_calls: None,
                    });
                }
            }
            MessageRole::System => {
                if let Some(content) = text_content {
                    messages.push(Self::build_text_message("system", content));
                }
            }
        }

        messages.extend(tool_messages);
        messages
    }

    fn build_openai_tools(tools: &[ToolSpec]) -> Option<Vec<OpenAiTool>> {
        map_tools_optional(tools, |tool| {
            let fields = ToolFields::from_tool(tool);
            OpenAiTool {
                r#type: "function",
                function: OpenAiToolDefinition {
                    name: fields.name,
                    description: fields.description,
                    parameters: fields.parameters,
                },
            }
        })
    }

    fn build_tools_request(
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> ChatRequest {
        let mut openai_messages = Vec::new();

        if let Some(sys) = system_prompt {
            openai_messages.push(Self::build_text_message(
                "system",
                scrub_secret_patterns(sys).into_owned(),
            ));
        }

        for provider_message in messages {
            openai_messages.extend(Self::map_provider_message(provider_message));
        }

        ChatRequest {
            model: model.to_string(),
            messages: openai_messages,
            temperature,
            tools: Self::build_openai_tools(tools),
            stream: None,
            stream_options: None,
        }
    }

    fn extract_text(chat_response: &ChatResponse) -> anyhow::Result<String> {
        chat_response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))
    }

    fn map_finish_reason(finish_reason: Option<&str>) -> StopReason {
        match finish_reason {
            Some("stop") => StopReason::EndTurn,
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            Some(_) | None => StopReason::Error,
        }
    }

    fn parse_tool_calls(
        tool_calls: Option<Vec<OpenAiToolCall>>,
    ) -> anyhow::Result<Vec<ContentBlock>> {
        tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tool_call| {
                let input: Value = serde_json::from_str(&tool_call.function.arguments)
                    .with_context(|| {
                        format!(
                            "OpenAI tool call arguments were not valid JSON for {}",
                            tool_call.function.name
                        )
                    })?;
                Ok(ContentBlock::ToolUse {
                    id: tool_call.id,
                    name: tool_call.function.name,
                    input,
                })
            })
            .collect()
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
        let auth_header = self.cached_auth_header.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", auth_header)
            .json(&request)
            .send()
            .await
            .context("OpenAI request failed")?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenAI", response).await);
        }

        response
            .json()
            .await
            .context("OpenAI response JSON decode failed")
    }

    async fn call_api_streaming(&self, request: &ChatRequest) -> anyhow::Result<reqwest::Response> {
        let auth_header = self.cached_auth_header.as_ref().ok_or_else(|| {
            anyhow::anyhow!("OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.")
        })?;

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", auth_header)
            .json(request)
            .send()
            .await
            .context("OpenAI request failed")?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenAI", response).await);
        }

        Ok(response)
    }

    async fn chat_with_tools_stream_impl(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        use crate::core::providers::streaming::StreamEvent;
        use futures_util::StreamExt;

        let request = ChatRequest {
            model: req.model,
            messages: {
                let mut openai_messages = Vec::new();

                if let Some(sys) = req.system_prompt {
                    openai_messages.push(Self::build_text_message(
                        "system",
                        scrub_secret_patterns(&sys).into_owned(),
                    ));
                }

                for provider_message in &req.messages {
                    openai_messages.extend(Self::map_provider_message(provider_message));
                }

                openai_messages
            },
            temperature: req.temperature,
            tools: Self::build_openai_tools(&req.tools),
            stream: Some(true),
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
        };

        let response = self.call_api_streaming(&request).await?;
        let mut byte_stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            let mut sse_buffer = SseBuffer::new();
            let mut sent_start = false;

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result?;
                sse_buffer.push_chunk(&chunk);

                while let Some(event_block) = sse_buffer.next_event_block() {
                    for data in parse_data_lines_without_done(&event_block) {
                        let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(data) else {
                            continue;
                        };

                        if !sent_start {
                            yield StreamEvent::ResponseStart {
                                model: chunk.model.clone(),
                            };
                            sent_start = true;
                        }

                        for choice in &chunk.choices {
                            if let Some(content) = &choice.delta.content
                                && !content.is_empty()
                            {
                                yield StreamEvent::TextDelta {
                                    text: content.clone(),
                                };
                            }

                            if let Some(tool_calls) = &choice.delta.tool_calls {
                                for tool_call in tool_calls {
                                    yield StreamEvent::ToolCallDelta {
                                        index: tool_call.index,
                                        id: tool_call.id.clone(),
                                        name: tool_call.function.as_ref().and_then(|f| f.name.clone()),
                                        input_json_delta: tool_call
                                            .function
                                            .as_ref()
                                            .and_then(|f| f.arguments.clone())
                                            .unwrap_or_default(),
                                    };
                                }
                            }

                            if let Some(finish) = choice.finish_reason.as_deref() {
                                let stop = Self::map_finish_reason(Some(finish));
                                let (input_t, output_t) = chunk
                                    .usage
                                    .as_ref()
                                    .map_or((None, None), |u| {
                                        (Some(u.prompt_tokens), Some(u.completion_tokens))
                                    });

                                yield StreamEvent::Done {
                                    stop_reason: Some(stop),
                                    input_tokens: input_t,
                                    output_tokens: output_t,
                                };
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
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
            ProviderResponse::with_usage(text, usage.prompt_tokens, usage.completion_tokens)
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
        let request = Self::build_tools_request(system_prompt, messages, tools, model, temperature);
        let chat_response = self.call_api_with_request(&request).await?;
        let choice = chat_response
            .choices
            .first()
            .ok_or_else(|| anyhow::anyhow!("No response from OpenAI"))?;

        let text = choice.message.content.clone().unwrap_or_default();
        let scrubbed_text = scrub_secret_patterns(&text).into_owned();
        let mut content_blocks = Self::parse_tool_calls(choice.message.tool_calls.clone())?;

        if !scrubbed_text.is_empty() {
            content_blocks.insert(
                0,
                ContentBlock::Text {
                    text: scrubbed_text.clone(),
                },
            );
        }

        let mut provider_response = if let Some(usage) = chat_response.usage {
            ProviderResponse::with_usage(
                scrubbed_text,
                usage.prompt_tokens,
                usage.completion_tokens,
            )
        } else {
            ProviderResponse::text_only(scrubbed_text)
        };

        provider_response.content_blocks = content_blocks;
        provider_response.stop_reason =
            Some(Self::map_finish_reason(choice.finish_reason.as_deref()));

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
        let req =
            OpenAiProvider::build_request(Some("You are AsteronIris"), "hello", "gpt-4o", 0.7);
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
}
