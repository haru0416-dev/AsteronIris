pub(super) mod compat;
pub(super) mod types;
#[cfg(test)]
use crate::core::providers::{ContentBlock, StopReason};
#[cfg(test)]
use crate::core::providers::{ImageSource, MessageRole, sse::parse_data_lines_without_done};
use crate::core::providers::{
    ProviderMessage, ProviderResponse, build_provider_client, streaming::ProviderStream,
    traits::Provider,
};
use crate::core::tools::traits::ToolSpec;
use compat as openai_compat;
use reqwest::Client;
use std::future::Future;
use std::pin::Pin;
#[cfg(test)]
use types::Message;
#[cfg(test)]
use types::OpenAiToolCall;
use types::{ChatRequest, ChatResponse};

pub struct OpenAiProvider {
    /// Pre-computed `"Bearer <key>"` header value (avoids `format!` per request).
    cached_auth_header: Option<String>,
    client: Client,
}

const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
const OPENAI_MISSING_API_KEY_MESSAGE: &str =
    "OpenAI API key not set. Set OPENAI_API_KEY or edit config.toml.";

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
        openai_compat::build_request(system_prompt, message, model, temperature)
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

    fn extract_text(chat_response: &ChatResponse) -> anyhow::Result<String> {
        openai_compat::extract_text(chat_response, "OpenAI")
    }

    #[cfg(test)]
    fn map_finish_reason(finish_reason: Option<&str>) -> StopReason {
        openai_compat::map_finish_reason(finish_reason)
    }

    #[cfg(test)]
    fn parse_tool_calls(
        tool_calls: Option<Vec<OpenAiToolCall>>,
    ) -> anyhow::Result<Vec<ContentBlock>> {
        openai_compat::parse_tool_calls(tool_calls, "OpenAI")
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
        openai_compat::send_chat_completions_json(
            &self.client,
            self.cached_auth_header.as_ref(),
            request,
            openai_compat::ChatCompletionsEndpoint {
                provider_name: "OpenAI",
                url: OPENAI_CHAT_COMPLETIONS_URL,
                missing_api_key_message: OPENAI_MISSING_API_KEY_MESSAGE,
                extra_headers: &[],
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
                provider_name: "OpenAI",
                url: OPENAI_CHAT_COMPLETIONS_URL,
                missing_api_key_message: OPENAI_MISSING_API_KEY_MESSAGE,
                extra_headers: &[],
            },
        )
        .await
    }

    async fn chat_with_tools_stream_impl(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderStream> {
        let request =
            openai_compat::build_stream_request(system_prompt, messages, tools, model, temperature);
        let response = self.call_api_streaming(&request).await?;
        Ok(openai_compat::sse_response_to_provider_stream(response))
    }
}

impl Provider for OpenAiProvider {
    fn chat_with_system<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let chat_response = self
                .call_api(system_prompt, message, model, temperature)
                .await?;
            Self::extract_text(&chat_response)
        })
    }

    fn chat_with_system_full<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let chat_response = self
                .call_api(system_prompt, message, model, temperature)
                .await?;

            openai_compat::build_text_provider_response(chat_response, "OpenAI")
        })
    }

    fn chat_with_tools<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        messages: &'a [ProviderMessage],
        tools: &'a [ToolSpec],
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let request =
                Self::build_tools_request(system_prompt, messages, tools, model, temperature);
            let chat_response = self.call_api_with_request(&request).await?;
            openai_compat::build_tool_provider_response(chat_response, "OpenAI")
        })
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

    fn chat_with_tools_stream<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        messages: &'a [ProviderMessage],
        tools: &'a [ToolSpec],
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send + 'a>> {
        Box::pin(async move {
            self.chat_with_tools_stream_impl(system_prompt, messages, tools, model, temperature)
                .await
        })
    }
}

#[cfg(test)]
mod tests;
