use crate::providers::{
    ContentBlock, MessageRole, ProviderMessage, ProviderResponse, StopReason,
    scrub_secret_patterns, traits::Provider,
};
use crate::tools::traits::ToolSpec;
use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct OpenAiProvider {
    /// Pre-computed `"Bearer <key>"` header value (avoids `format!` per request).
    cached_auth_header: Option<String>,
    client: Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiTool {
    r#type: &'static str,
    function: OpenAiToolDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAiToolDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    r#type: String,
    function: OpenAiToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAiToolCallFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<&str>) -> Self {
        Self {
            cached_auth_header: api_key.map(|k| format!("Bearer {k}")),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .pool_max_idle_per_host(10)
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .tcp_keepalive(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| Client::new()),
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
                content: Some(sys.to_string()),
                tool_call_id: None,
                tool_calls: None,
            });
        }

        messages.push(Message {
            role: "user",
            content: Some(message.to_string()),
            tool_call_id: None,
            tool_calls: None,
        });

        ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            tools: None,
        }
    }

    fn build_text_message(role: &'static str, content: String) -> Message {
        Message {
            role,
            content: Some(content),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    fn map_provider_message(provider_message: &ProviderMessage) -> Vec<Message> {
        let mut text_parts = Vec::new();
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
                        content: Some(scrub_secret_patterns(content).into_owned()),
                        tool_call_id: Some(tool_use_id.clone()),
                        tool_calls: None,
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
                        content: text_content,
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
                if let Some(content) = text_content {
                    messages.push(Self::build_text_message("user", content));
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
        if tools.is_empty() {
            None
        } else {
            Some(
                tools
                    .iter()
                    .map(|tool| OpenAiTool {
                        r#type: "function",
                        function: OpenAiToolDefinition {
                            name: tool.name.clone(),
                            description: tool.description.clone(),
                            parameters: tool.parameters.clone(),
                        },
                    })
                    .collect(),
            )
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Provider;

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
    fn supports_tool_calling_returns_true() {
        let provider = OpenAiProvider::new(Some("sk-test"));
        assert!(provider.supports_tool_calling());
    }
}
