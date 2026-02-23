#[cfg(test)]
use super::openai_types::Message;
use super::{
    openai_compat,
    openai_types::{ChatRequest, ChatResponse},
};
#[cfg(test)]
use crate::core::providers::sse::parse_data_lines_without_done;
use crate::core::providers::{
    ProviderMessage, ProviderResponse, build_provider_client,
    streaming::{ProviderChatRequest, ProviderStream},
    traits::Provider,
};
use crate::core::tools::traits::ToolSpec;
use async_trait::async_trait;
use reqwest::Client;

pub struct OpenRouterProvider {
    /// Pre-computed `"Bearer <key>"` header value (avoids `format!` per request).
    cached_auth_header: Option<String>,
    client: Client,
}

const OPENROUTER_CHAT_COMPLETIONS_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const OPENROUTER_MISSING_API_KEY_MESSAGE: &str =
    "OpenRouter API key not set. Run `asteroniris onboard` or set OPENROUTER_API_KEY env var.";
const OPENROUTER_EXTRA_HEADERS: [(&str, &str); 2] = [
    (
        "HTTP-Referer",
        "https://github.com/haru0416-dev/AsteronIris",
    ),
    ("X-Title", "AsteronIris"),
];

impl OpenRouterProvider {
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
        openai_compat::build_request(system_prompt, message, model, temperature)
    }

    fn extract_text(chat_response: &ChatResponse) -> anyhow::Result<String> {
        openai_compat::extract_text(chat_response, "OpenRouter")
    }

    #[cfg(test)]
    fn map_provider_message(provider_message: &ProviderMessage) -> Vec<Message> {
        openai_compat::map_provider_message(provider_message)
    }

    fn build_tools_request(
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> ChatRequest {
        openai_compat::build_tools_request(system_prompt, messages, tools, model, temperature)
    }

    async fn call_api_with_request(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        openai_compat::send_chat_completions_json(
            &self.client,
            self.cached_auth_header.as_ref(),
            request,
            openai_compat::ChatCompletionsEndpoint {
                provider_name: "OpenRouter",
                url: OPENROUTER_CHAT_COMPLETIONS_URL,
                missing_api_key_message: OPENROUTER_MISSING_API_KEY_MESSAGE,
                extra_headers: &OPENROUTER_EXTRA_HEADERS,
            },
        )
        .await
    }

    async fn call_api_streaming(&self, request: &ChatRequest) -> anyhow::Result<reqwest::Response> {
        openai_compat::send_chat_completions_raw(
            &self.client,
            self.cached_auth_header.as_ref(),
            request,
            openai_compat::ChatCompletionsEndpoint {
                provider_name: "OpenRouter",
                url: OPENROUTER_CHAT_COMPLETIONS_URL,
                missing_api_key_message: OPENROUTER_MISSING_API_KEY_MESSAGE,
                extra_headers: &OPENROUTER_EXTRA_HEADERS,
            },
        )
        .await
    }

    async fn chat_with_tools_stream_impl(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        let request = openai_compat::build_stream_request(req);
        let response = self.call_api_streaming(&request).await?;
        Ok(openai_compat::sse_response_to_provider_stream(response))
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
}

#[async_trait]
impl Provider for OpenRouterProvider {
    async fn warmup(&self) -> anyhow::Result<()> {
        // Hit a lightweight endpoint to establish TLS + HTTP/2 connection pool.
        // This prevents the first real chat request from timing out on cold start.
        if let Some(auth_header) = self.cached_auth_header.as_ref() {
            self.client
                .get("https://openrouter.ai/api/v1/auth/key")
                .header("Authorization", auth_header)
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }

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
        openai_compat::build_text_provider_response(chat_response, "OpenRouter")
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
        openai_compat::build_tool_provider_response(chat_response, "OpenRouter")
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
mod tests;
