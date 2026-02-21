//! Generic OpenAI-compatible provider.
//! Most LLM APIs follow the same `/v1/chat/completions` format.
//! This module provides a single implementation that works for all of them.

use super::sanitize_api_error;
use crate::core::providers::{
    ProviderMessage, ProviderResponse, build_provider_client,
    fallback_tools::{augment_system_prompt_with_tools, build_fallback_response},
    traits::{Provider, messages_to_text},
};
use crate::core::tools::traits::ToolSpec;
use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// A provider that speaks the OpenAI-compatible chat completions API.
/// Used by: Venice, Vercel AI Gateway, Cloudflare AI Gateway, Moonshot,
/// Synthetic, `OpenCode` Zen, `Z.AI`, `GLM`, `MiniMax`, Bedrock, Qianfan, Groq, Mistral, `xAI`, etc.
pub struct OpenAiCompatibleProvider {
    pub(crate) name: String,
    #[allow(dead_code)] // Retained for provider diagnostics/debug inspection
    pub(crate) base_url: String,
    pub(crate) api_key: Option<String>,
    #[allow(dead_code)] // Retained for provider diagnostics/debug inspection
    pub(crate) auth_header: AuthStyle,
    /// Pre-computed `(header_name, header_value)` for auth (avoids `format!` per request).
    cached_auth: Option<(String, String)>,
    /// Pre-computed chat completions URL (avoids `format!` per request).
    cached_chat_url: String,
    /// Pre-computed responses API URL (avoids `format!` per request).
    cached_responses_url: String,
    client: Client,
}

/// How the provider expects the API key to be sent.
#[derive(Debug, Clone)]
pub enum AuthStyle {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// `x-api-key: <key>` (used by some Chinese providers)
    XApiKey,
    /// Custom header name
    Custom(String),
}

impl OpenAiCompatibleProvider {
    pub fn new(name: &str, base_url: &str, api_key: Option<&str>, auth_style: AuthStyle) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        let cached_chat_url = if base_url.contains("chat/completions") {
            base_url.clone()
        } else {
            format!("{base_url}/chat/completions")
        };
        let cached_responses_url = if base_url.contains("responses") {
            base_url.clone()
        } else {
            format!("{base_url}/v1/responses")
        };

        let cached_auth = api_key.map(|k| match &auth_style {
            AuthStyle::Bearer => ("Authorization".to_string(), format!("Bearer {k}")),
            AuthStyle::XApiKey => ("x-api-key".to_string(), k.to_string()),
            AuthStyle::Custom(header) => (header.clone(), k.to_string()),
        });

        Self {
            name: name.to_string(),
            base_url,
            api_key: api_key.map(ToString::to_string),
            auth_header: auth_style,
            cached_auth,
            cached_chat_url,
            cached_responses_url,
            client: build_provider_client(),
        }
    }

    fn chat_completions_url(&self) -> &str {
        &self.cached_chat_url
    }

    fn responses_url(&self) -> &str {
        &self.cached_responses_url
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<ChatUsage>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ResponsesInput {
    role: &'static str,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output: Vec<ResponsesOutput>,
    #[serde(default)]
    output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutput {
    #[serde(default)]
    content: Vec<ResponsesContent>,
}

#[derive(Debug, Deserialize)]
struct ResponsesContent {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

fn first_nonempty(text: Option<&str>) -> Option<String> {
    text.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn extract_responses_text(response: &ResponsesResponse) -> Option<String> {
    if let Some(text) = first_nonempty(response.output_text.as_deref()) {
        return Some(text);
    }

    for item in &response.output {
        for content in &item.content {
            if content.kind.as_deref() == Some("output_text")
                && let Some(text) = first_nonempty(content.text.as_deref())
            {
                return Some(text);
            }
        }
    }

    for item in &response.output {
        for content in &item.content {
            if let Some(text) = first_nonempty(content.text.as_deref()) {
                return Some(text);
            }
        }
    }

    None
}

fn extract_chat_text(response: &ChatResponse, provider_name: &str) -> anyhow::Result<String> {
    response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| anyhow::anyhow!("No response from {provider_name}"))
}

impl OpenAiCompatibleProvider {
    fn apply_auth_header(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some((name, value)) = &self.cached_auth {
            req.header(name, value)
        } else {
            req
        }
    }

    async fn chat_via_responses(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
    ) -> anyhow::Result<ProviderResponse> {
        let request = ResponsesRequest {
            model: model.to_string(),
            input: vec![ResponsesInput {
                role: "user",
                content: message.to_string(),
            }],
            instructions: system_prompt.map(str::to_string),
            stream: Some(false),
        };

        let url = self.responses_url();

        let response = self
            .apply_auth_header(self.client.post(url).json(&request))
            .send()
            .await
            .with_context(|| format!("{} Responses API request failed", self.name))?;

        if !response.status().is_success() {
            let error = response.text().await?;
            let sanitized = sanitize_api_error(&error);
            anyhow::bail!("{} Responses API error: {sanitized}", self.name);
        }

        let responses: ResponsesResponse = response
            .json()
            .await
            .with_context(|| format!("{} Responses API JSON decode failed", self.name))?;

        let text = extract_responses_text(&responses)
            .ok_or_else(|| anyhow::anyhow!("No response from {} Responses API", self.name))?;
        Ok(ProviderResponse::text_only(text))
    }

    async fn call_chat_completions(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = self.chat_completions_url();

        let response = self
            .apply_auth_header(self.client.post(url).json(request))
            .send()
            .await
            .with_context(|| format!("{} chat completions request failed", self.name))?;

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;
            let sanitized_error = sanitize_api_error(&error);

            if status == reqwest::StatusCode::NOT_FOUND {
                anyhow::bail!("NOT_FOUND_FALLBACK::{sanitized_error}");
            }

            anyhow::bail!("{} API error: {sanitized_error}", self.name);
        }

        response
            .json()
            .await
            .with_context(|| format!("{} chat completions JSON decode failed", self.name))
    }

    async fn chat_with_system_internal(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        if self.api_key.is_none() {
            anyhow::bail!(
                "{} API key not set. Run `asteroniris onboard` or set the appropriate env var.",
                self.name
            );
        }

        let capacity = if system_prompt.is_some() { 2 } else { 1 };
        let mut messages = Vec::with_capacity(capacity);

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system",
                content: sys.to_string(),
            });
        }

        messages.push(Message {
            role: "user",
            content: message.to_string(),
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
        };

        match self.call_chat_completions(&request).await {
            Ok(chat_response) => {
                let text = extract_chat_text(&chat_response, &self.name)?;
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
            Err(error) => {
                let error_text = error.to_string();
                if let Some((_, sanitized_error)) = error_text.split_once("NOT_FOUND_FALLBACK::") {
                    return self
                        .chat_via_responses(system_prompt, message, model)
                        .await
                        .map_err(|responses_err| {
                            anyhow::anyhow!(
                                "{} API error: {sanitized_error} (chat completions unavailable; responses fallback failed: {responses_err})",
                                self.name
                            )
                        });
                }

                Err(error)
            }
        }
    }

    fn prepare_fallback_input(
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
    ) -> (String, String) {
        let augmented_prompt = augment_system_prompt_with_tools(system_prompt.unwrap_or(""), tools);
        let text = messages_to_text(messages);
        (augmented_prompt, text)
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.chat_with_system_internal(system_prompt, message, model, temperature)
            .await
            .map(|response| response.text)
    }

    async fn chat_with_system_full(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        self.chat_with_system_internal(system_prompt, message, model, temperature)
            .await
    }

    async fn chat_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let (augmented_prompt, text) = Self::prepare_fallback_input(system_prompt, messages, tools);
        let response = self
            .chat_with_system_full(Some(&augmented_prompt), &text, model, temperature)
            .await?;
        Ok(build_fallback_response(response, tools))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::providers::{ContentBlock, MessageRole, Provider, ProviderMessage};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_provider(name: &str, url: &str, key: Option<&str>) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(name, url, key, AuthStyle::Bearer)
    }

    #[test]
    fn creates_with_key() {
        let p = make_provider("venice", "https://api.venice.ai", Some("vn-key"));
        assert_eq!(p.name, "venice");
        assert_eq!(p.base_url, "https://api.venice.ai");
        assert_eq!(p.api_key.as_deref(), Some("vn-key"));
    }

    #[test]
    fn creates_without_key() {
        let p = make_provider("test", "https://example.com", None);
        assert!(p.api_key.is_none());
    }

    #[test]
    fn strips_trailing_slash() {
        let p = make_provider("test", "https://example.com/", None);
        assert_eq!(p.base_url, "https://example.com");
    }

    #[tokio::test]
    async fn chat_fails_without_key() {
        let p = make_provider("Venice", "https://api.venice.ai", None);
        let result = p
            .chat_with_system(None, "hello", "llama-3.3-70b", 0.7)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Venice API key not set")
        );
    }

    #[test]
    fn request_serializes_correctly() {
        let req = ChatRequest {
            model: "llama-3.3-70b".to_string(),
            messages: vec![
                Message {
                    role: "system",
                    content: "You are AsteronIris".to_string(),
                },
                Message {
                    role: "user",
                    content: "hello".to_string(),
                },
            ],
            temperature: 0.7,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("llama-3.3-70b"));
        assert!(json.contains("system"));
        assert!(json.contains("user"));
    }

    #[test]
    fn response_deserializes() {
        let json = r#"{"choices":[{"message":{"content":"Hello from Venice!"}}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices[0].message.content, "Hello from Venice!");
    }

    #[test]
    fn response_empty_choices() {
        let json = r#"{"choices":[]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.choices.is_empty());
    }

    #[test]
    fn x_api_key_auth_style() {
        let p = OpenAiCompatibleProvider::new(
            "moonshot",
            "https://api.moonshot.cn",
            Some("ms-key"),
            AuthStyle::XApiKey,
        );
        assert!(matches!(p.auth_header, AuthStyle::XApiKey));
    }

    #[test]
    fn custom_auth_style() {
        let p = OpenAiCompatibleProvider::new(
            "custom",
            "https://api.example.com",
            Some("key"),
            AuthStyle::Custom("X-Custom-Key".into()),
        );
        assert!(matches!(p.auth_header, AuthStyle::Custom(_)));
    }

    #[tokio::test]
    async fn all_compatible_providers_fail_without_key() {
        let providers = vec![
            make_provider("Venice", "https://api.venice.ai", None),
            make_provider("Moonshot", "https://api.moonshot.cn", None),
            make_provider("GLM", "https://open.bigmodel.cn", None),
            make_provider("MiniMax", "https://api.minimax.chat", None),
            make_provider("Groq", "https://api.groq.com/openai", None),
            make_provider("Mistral", "https://api.mistral.ai", None),
            make_provider("xAI", "https://api.x.ai", None),
        ];

        for p in providers {
            let result = p.chat_with_system(None, "test", "model", 0.7).await;
            assert!(result.is_err(), "{} should fail without key", p.name);
            assert!(
                result.unwrap_err().to_string().contains("API key not set"),
                "{} error should mention key",
                p.name
            );
        }
    }

    #[tokio::test]
    async fn chat_error_messages_redact_sensitive_fields() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string(
                "{\"error\":\"invalid credentials api_key=raw-secret-123 access_token=eyJhbGciOiJIUzI1Ni\"}",
            ))
            .mount(&server)
            .await;

        let provider = make_provider("MockProvider", &server.uri(), Some("key"));
        let err = provider
            .chat_with_system(None, "hello", "test-model", 0.1)
            .await
            .unwrap_err()
            .to_string();

        assert!(!err.contains("raw-secret-123"));
        assert!(!err.contains("eyJhbGciOiJIUzI1Ni"));
        assert!(err.contains("[REDACTED]"));
    }

    #[test]
    fn responses_extracts_top_level_output_text() {
        let json = r#"{"output_text":"Hello from top-level","output":[]}"#;
        let response: ResponsesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_responses_text(&response).as_deref(),
            Some("Hello from top-level")
        );
    }

    #[test]
    fn responses_extracts_nested_output_text() {
        let json =
            r#"{"output":[{"content":[{"type":"output_text","text":"Hello from nested"}]}]}"#;
        let response: ResponsesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_responses_text(&response).as_deref(),
            Some("Hello from nested")
        );
    }

    #[test]
    fn responses_extracts_any_text_as_fallback() {
        let json = r#"{"output":[{"content":[{"type":"message","text":"Fallback text"}]}]}"#;
        let response: ResponsesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_responses_text(&response).as_deref(),
            Some("Fallback text")
        );
    }

    // ══════════════════════════════════════════════════════════
    // Custom endpoint path tests (Issue #114)
    // ══════════════════════════════════════════════════════════

    #[test]
    fn chat_completions_url_standard_openai() {
        // Standard OpenAI-compatible providers get /chat/completions appended
        let p = make_provider("openai", "https://api.openai.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_trailing_slash() {
        // Trailing slash is stripped, then /chat/completions appended
        let p = make_provider("test", "https://api.example.com/v1/", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_volcengine_ark() {
        // VolcEngine ARK uses custom path - should use as-is
        let p = make_provider(
            "volcengine",
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_custom_full_endpoint() {
        // Custom provider with full endpoint path
        let p = make_provider(
            "custom",
            "https://my-api.example.com/v2/llm/chat/completions",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://my-api.example.com/v2/llm/chat/completions"
        );
    }

    #[test]
    fn responses_url_standard() {
        // Standard providers get /v1/responses appended
        let p = make_provider("test", "https://api.example.com", None);
        assert_eq!(p.responses_url(), "https://api.example.com/v1/responses");
    }

    #[test]
    fn responses_url_custom_full_endpoint() {
        // Custom provider with full responses endpoint
        let p = make_provider(
            "custom",
            "https://my-api.example.com/api/v2/responses",
            None,
        );
        assert_eq!(
            p.responses_url(),
            "https://my-api.example.com/api/v2/responses"
        );
    }

    #[test]
    fn chat_completions_url_without_v1() {
        // Provider configured without /v1 in base URL
        let p = make_provider("test", "https://api.example.com", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_base_with_v1() {
        // Provider configured with /v1 in base URL
        let p = make_provider("test", "https://api.example.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    // ══════════════════════════════════════════════════════════
    // Provider-specific endpoint tests (Issue #167)
    // ══════════════════════════════════════════════════════════

    #[test]
    fn chat_completions_url_zai() {
        // Z.AI uses /api/paas/v4 base path
        let p = make_provider("zai", "https://api.z.ai/api/paas/v4", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.z.ai/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_glm() {
        // GLM (BigModel) uses /api/paas/v4 base path
        let p = make_provider("glm", "https://open.bigmodel.cn/api/paas/v4", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_opencode() {
        // OpenCode Zen uses /zen/v1 base path
        let p = make_provider("opencode", "https://opencode.ai/zen/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://opencode.ai/zen/v1/chat/completions"
        );
    }

    #[test]
    fn fallback_input_includes_tool_schema_in_augmented_prompt() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "read src/lib.rs".to_string(),
            }],
        }];
        let tools = vec![ToolSpec {
            name: "file_read".to_string(),
            description: "Read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"}
                }
            }),
        }];

        let (prompt, text) = OpenAiCompatibleProvider::prepare_fallback_input(
            Some("System prompt"),
            &messages,
            &tools,
        );

        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("file_read: Read a file"));
        assert!(text.contains("User: read src/lib.rs"));
    }

    #[test]
    fn supports_tool_calling_returns_false() {
        let provider = make_provider("test", "https://api.example.com", Some("key"));
        assert!(!provider.supports_tool_calling());
    }
}
