#[cfg(test)]
use super::openai_types::Message;
use super::{
    openai_compat,
    openai_types::{ChatRequest, ChatResponse, OpenAiToolCall},
};
use crate::core::providers::{
    ContentBlock, ProviderMessage, ProviderResponse, StopReason, build_provider_client,
    scrub_secret_patterns,
    streaming::{ProviderChatRequest, ProviderStream},
    traits::Provider,
};
#[cfg(test)]
use crate::core::providers::{ImageSource, MessageRole, sse::parse_data_lines_without_done};
use crate::core::tools::traits::ToolSpec;
use async_trait::async_trait;
use reqwest::Client;

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

    fn map_finish_reason(finish_reason: Option<&str>) -> StopReason {
        openai_compat::map_finish_reason(finish_reason)
    }

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
            .map_err(|error| anyhow::anyhow!("OpenAI request failed: {error}"))?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenAI", response).await);
        }

        response
            .json()
            .await
            .map_err(|error| anyhow::anyhow!("OpenAI response JSON decode failed: {error}"))
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
            .map_err(|error| anyhow::anyhow!("OpenAI request failed: {error}"))?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenAI", response).await);
        }

        Ok(response)
    }

    async fn chat_with_tools_stream_impl(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        let request = openai_compat::build_stream_request(req);
        let response = self.call_api_streaming(&request).await?;
        Ok(openai_compat::sse_response_to_provider_stream(response))
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
mod tests;
