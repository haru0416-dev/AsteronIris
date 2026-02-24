//! Google Gemini provider with support for:
//! - Direct API key (`GEMINI_API_KEY` env var or config)
//! - Gemini CLI OAuth tokens (reuse existing ~/.gemini/ authentication)
//! - Google Cloud ADC (`GOOGLE_APPLICATION_CREDENTIALS`)

use crate::llm::{
    build_provider_client, sanitize_api_error, scrub_secret_patterns,
    sse::{SseBuffer, parse_data_lines},
    streaming::ProviderStream,
    tool_convert::{ToolFields, map_tools_optional},
    traits::{Provider, ProviderCapabilities},
    types::{
        ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
    },
};
use crate::tools::ToolSpec;
use directories::UserDirs;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

mod types;
use types::{
    Candidate, Content, GeminiFileData, GeminiFunctionCall, GeminiFunctionDeclaration,
    GeminiFunctionResponse, GeminiInlineData, GeminiTool, GenerateContentRequest,
    GenerateContentResponse, GenerationConfig, Part, ResponsePart,
};

/// Gemini provider supporting multiple authentication methods.
pub struct GeminiProvider {
    api_key: Option<String>,
    client: Client,
}

// ══════════════════════════════════════════════════════════════════════════════
// GEMINI CLI TOKEN STRUCTURES
// ══════════════════════════════════════════════════════════════════════════════

/// OAuth token stored by Gemini CLI in `~/.gemini/oauth_creds.json`
#[derive(Debug, Deserialize)]
struct GeminiCliOAuthCreds {
    access_token: Option<String>,
    #[allow(dead_code)] // Retained for future Gemini OAuth token refresh support
    refresh_token: Option<String>,
    expiry: Option<String>,
}

impl GeminiProvider {
    /// Create a new Gemini provider.
    ///
    /// Authentication priority:
    /// 1. Explicit API key passed in
    /// 2. `GEMINI_API_KEY` environment variable
    /// 3. `GOOGLE_API_KEY` environment variable
    /// 4. Gemini CLI OAuth tokens (`~/.gemini/oauth_creds.json`)
    pub fn new(api_key: Option<&str>) -> Self {
        let resolved_key = api_key
            .map(String::from)
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok())
            .or_else(Self::try_load_gemini_cli_token);

        Self {
            api_key: resolved_key,
            client: build_provider_client(),
        }
    }

    /// Try to load OAuth access token from Gemini CLI's cached credentials.
    /// Location: `~/.gemini/oauth_creds.json`
    fn try_load_gemini_cli_token() -> Option<String> {
        let gemini_dir = Self::gemini_cli_dir()?;
        let creds_path = gemini_dir.join("oauth_creds.json");

        if !creds_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&creds_path).ok()?;
        let creds: GeminiCliOAuthCreds = serde_json::from_str(&content).ok()?;

        // Check if token is expired (basic check)
        if let Some(ref expiry) = creds.expiry
            && let Ok(expiry_time) = chrono::DateTime::parse_from_rfc3339(expiry)
            && expiry_time < chrono::Utc::now()
        {
            tracing::debug!("Gemini CLI OAuth token expired, skipping");
            return None;
        }

        creds.access_token
    }

    /// Get the Gemini CLI config directory (~/.gemini)
    fn gemini_cli_dir() -> Option<PathBuf> {
        UserDirs::new().map(|u| u.home_dir().join(".gemini"))
    }

    /// Check if Gemini CLI is configured and has valid credentials
    pub fn has_cli_credentials() -> bool {
        Self::try_load_gemini_cli_token().is_some()
    }

    /// Check if any Gemini authentication is available
    pub fn has_any_auth() -> bool {
        std::env::var("GEMINI_API_KEY").is_ok()
            || std::env::var("GOOGLE_API_KEY").is_ok()
            || Self::has_cli_credentials()
    }

    /// Get authentication source description for diagnostics
    pub fn auth_source(&self) -> &'static str {
        if self.api_key.is_none() {
            return "none";
        }
        if std::env::var("GEMINI_API_KEY").is_ok() {
            return "GEMINI_API_KEY env var";
        }
        if std::env::var("GOOGLE_API_KEY").is_ok() {
            return "GOOGLE_API_KEY env var";
        }
        if Self::has_cli_credentials() {
            return "Gemini CLI OAuth";
        }
        "config"
    }

    fn build_request(
        system_prompt: Option<&str>,
        message: &str,
        temperature: f64,
    ) -> GenerateContentRequest {
        let system_instruction = system_prompt.map(|sys| Content {
            role: None,
            parts: vec![Part::text(scrub_secret_patterns(sys).into_owned())],
        });

        GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part::text(scrub_secret_patterns(message).into_owned())],
            }],
            system_instruction,
            tools: None,
            generation_config: GenerationConfig {
                temperature,
                max_output_tokens: 8192,
            },
        }
    }

    fn model_name(model: &str) -> String {
        if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{model}")
        }
    }

    fn api_key(&self) -> anyhow::Result<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "Gemini API key not found. Options:\n\
                 1. Set GEMINI_API_KEY env var\n\
                 2. Run `gemini` CLI to authenticate (tokens will be reused)\n\
                 3. Get an API key from https://aistudio.google.com/app/apikey\n\
                 4. Run `asteroniris onboard` to configure"
            )
        })
    }

    async fn send_api_request(
        &self,
        url: String,
        request: &GenerateContentRequest,
    ) -> anyhow::Result<reqwest::Response> {
        let response = self.client.post(url).json(request).send().await?;
        Self::ensure_success_status(response).await
    }

    async fn ensure_success_status(
        response: reqwest::Response,
    ) -> anyhow::Result<reqwest::Response> {
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            let sanitized_error = sanitize_api_error(&error_text);
            anyhow::bail!("Gemini API error ({status}): {sanitized_error}");
        }

        Ok(response)
    }

    fn extract_text(result: &GenerateContentResponse) -> anyhow::Result<String> {
        let text = result
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .map(|candidate| {
                let mut out = String::new();
                for part in &candidate.content.parts {
                    if let Some(t) = &part.text {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t);
                    }
                }
                out
            })
            .unwrap_or_default();

        if text.is_empty() {
            anyhow::bail!("No response from Gemini");
        }

        Ok(text)
    }

    fn build_gemini_tools(tools: &[ToolSpec]) -> Option<Vec<GeminiTool>> {
        map_tools_optional(tools, |tool| {
            let fields = ToolFields::from_tool_with_description(
                tool,
                scrub_secret_patterns(&tool.description).into_owned(),
            );

            GeminiFunctionDeclaration {
                name: fields.name,
                description: fields.description,
                parameters: fields.parameters,
            }
        })
        .map(|function_declarations| {
            vec![GeminiTool {
                function_declarations,
            }]
        })
    }

    fn map_provider_message(
        provider_message: &ProviderMessage,
        tool_id_to_name: &HashMap<String, String>,
    ) -> Content {
        let role = match provider_message.role {
            MessageRole::Assistant => "model",
            MessageRole::User | MessageRole::System => "user",
        }
        .to_string();

        let parts = provider_message
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => Part::text(scrub_secret_patterns(text).into_owned()),
                ContentBlock::ToolUse { id, name, input } => {
                    let args = if input.is_object() {
                        input.clone()
                    } else {
                        let mut wrapped = Map::new();
                        wrapped.insert("input".to_string(), input.clone());
                        Value::Object(wrapped)
                    };
                    Part::function_call(GeminiFunctionCall {
                        name: name.clone(),
                        args,
                        id: Some(id.clone()),
                    })
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let tool_name = tool_id_to_name
                        .get(tool_use_id)
                        .cloned()
                        .unwrap_or_else(|| "tool".to_string());
                    Part::function_response(GeminiFunctionResponse {
                        name: tool_name,
                        response: serde_json::json!({
                            "tool_use_id": tool_use_id,
                            "content": scrub_secret_patterns(content).into_owned(),
                            "is_error": is_error,
                        }),
                    })
                }
                ContentBlock::Image { source } => match source {
                    ImageSource::Base64 { media_type, data } => {
                        Part::inline_data(GeminiInlineData {
                            mime_type: media_type.clone(),
                            data: data.clone(),
                        })
                    }
                    ImageSource::Url { url } => Part::file_data(GeminiFileData {
                        mime_type: String::new(),
                        file_uri: url.clone(),
                    }),
                },
            })
            .collect();

        Content {
            role: Some(role),
            parts,
        }
    }

    fn build_tools_request(
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        temperature: f64,
    ) -> GenerateContentRequest {
        let tool_id_to_name = messages
            .iter()
            .flat_map(|message| message.content.iter())
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, .. } => Some((id.clone(), name.clone())),
                ContentBlock::Text { .. }
                | ContentBlock::ToolResult { .. }
                | ContentBlock::Image { .. } => None,
            })
            .collect::<HashMap<_, _>>();

        GenerateContentRequest {
            contents: messages
                .iter()
                .map(|message| Self::map_provider_message(message, &tool_id_to_name))
                .collect(),
            system_instruction: system_prompt.map(|system| Content {
                role: None,
                parts: vec![Part::text(scrub_secret_patterns(system).into_owned())],
            }),
            tools: Self::build_gemini_tools(tools),
            generation_config: GenerationConfig {
                temperature,
                max_output_tokens: 8192,
            },
        }
    }

    fn map_stop_reason(candidate: &Candidate) -> StopReason {
        if candidate
            .content
            .parts
            .iter()
            .any(|part| part.function_call.is_some())
        {
            return StopReason::ToolUse;
        }

        match candidate.finish_reason.as_deref() {
            Some("STOP") => StopReason::EndTurn,
            Some("FUNCTION_CALL") => StopReason::ToolUse,
            Some("MAX_TOKENS") => StopReason::MaxTokens,
            Some(_) | None => StopReason::Error,
        }
    }

    fn parse_content_blocks(parts: &[ResponsePart]) -> Vec<ContentBlock> {
        let mut tool_call_index = 1usize;
        let mut blocks = Vec::new();

        for part in parts {
            if let Some(text) = &part.text {
                let scrubbed = scrub_secret_patterns(text).into_owned();
                if !scrubbed.is_empty() {
                    blocks.push(ContentBlock::Text { text: scrubbed });
                }
            }

            if let Some(function_call) = &part.function_call {
                let input = if function_call.args.is_object() {
                    function_call.args.clone()
                } else {
                    let mut wrapped = Map::new();
                    wrapped.insert("input".to_string(), function_call.args.clone());
                    Value::Object(wrapped)
                };
                let id = function_call
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("gemini_call_{tool_call_index}"));
                tool_call_index += 1;
                blocks.push(ContentBlock::ToolUse {
                    id,
                    name: function_call.name.clone(),
                    input,
                });
            }
        }

        blocks
    }

    async fn call_api_with_request(
        &self,
        model: &str,
        request: &GenerateContentRequest,
    ) -> anyhow::Result<GenerateContentResponse> {
        let api_key = self.api_key()?;

        let model_name = Self::model_name(model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{model_name}:generateContent?key={api_key}"
        );

        let response = self.send_api_request(url, request).await?;

        let result: GenerateContentResponse = response.json().await?;

        if let Some(err) = result.error.as_ref() {
            anyhow::bail!("Gemini API error: {}", sanitize_api_error(&err.message));
        }

        Ok(result)
    }

    async fn call_api_streaming(
        &self,
        model: &str,
        request: &GenerateContentRequest,
    ) -> anyhow::Result<reqwest::Response> {
        let api_key = self.api_key()?;

        let model_name = Self::model_name(model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{model_name}:streamGenerateContent?key={api_key}&alt=sse"
        );

        self.send_api_request(url, request).await
    }

    async fn chat_with_tools_stream_impl(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderStream> {
        use crate::llm::streaming::StreamEvent;
        use futures_util::StreamExt;

        let request = Self::build_tools_request(system_prompt, messages, tools, temperature);

        let response = self.call_api_streaming(model, &request).await?;
        let mut byte_stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            let mut sse_buffer = SseBuffer::new();
            let mut sent_start = false;
            let mut tool_call_index = 1usize;

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result?;
                sse_buffer.push_chunk(&chunk);

                while let Some(event_block) = sse_buffer.next_event_block() {
                    for data in parse_data_lines(&event_block) {
                        let Ok(gen_response) = serde_json::from_str::<GenerateContentResponse>(data) else {
                            continue;
                        };

                        if let Some(err) = gen_response.error.as_ref() {
                            Err(anyhow::anyhow!(
                                "Gemini API error: {}",
                                sanitize_api_error(&err.message)
                            ))?;
                        }

                        if !sent_start {
                            yield StreamEvent::ResponseStart { model: None };
                            sent_start = true;
                        }

                        if let Some(candidates) = &gen_response.candidates {
                            for candidate in candidates {
                                for part in &candidate.content.parts {
                                    if let Some(delta_text) = &part.text
                                        && !delta_text.is_empty()
                                    {
                                        yield StreamEvent::TextDelta {
                                            text: delta_text.clone(),
                                        };
                                    }

                                    if let Some(fc) = &part.function_call {
                                        let id = fc.id.clone().unwrap_or_else(|| {
                                            let generated = format!("gemini_call_{tool_call_index}");
                                            tool_call_index += 1;
                                            generated
                                        });
                                        let input = if fc.args.is_object() {
                                            fc.args.clone()
                                        } else {
                                            serde_json::json!({"input": fc.args})
                                        };

                                        yield StreamEvent::ToolCallComplete {
                                            id,
                                            name: fc.name.clone(),
                                            input,
                                        };
                                    }
                                }

                                if candidate.finish_reason.is_some() {
                                    let stop_reason = Self::map_stop_reason(candidate);
                                    let (input_tokens, output_tokens) = gen_response
                                        .usage_metadata
                                        .as_ref()
                                        .map_or((None, None), |usage| {
                                            (
                                                Some(usage.prompt_token_count),
                                                Some(usage.candidates_token_count),
                                            )
                                        });

                                    yield StreamEvent::Done {
                                        stop_reason: Some(stop_reason),
                                        input_tokens,
                                        output_tokens,
                                    };
                                }
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn call_api(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<GenerateContentResponse> {
        let request = Self::build_request(system_prompt, message, temperature);
        self.call_api_with_request(model, &request).await
    }
}

impl Part {
    fn text(text: String) -> Self {
        Self {
            text: Some(text),
            function_call: None,
            function_response: None,
            inline_data: None,
            file_data: None,
        }
    }

    fn function_call(function_call: GeminiFunctionCall) -> Self {
        Self {
            text: None,
            function_call: Some(function_call),
            function_response: None,
            inline_data: None,
            file_data: None,
        }
    }

    fn function_response(function_response: GeminiFunctionResponse) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: Some(function_response),
            inline_data: None,
            file_data: None,
        }
    }

    fn inline_data(data: GeminiInlineData) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: None,
            inline_data: Some(data),
            file_data: None,
        }
    }

    fn file_data(data: GeminiFileData) -> Self {
        Self {
            text: None,
            function_call: None,
            function_response: None,
            inline_data: None,
            file_data: Some(data),
        }
    }
}

impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            tool_calling: true,
            streaming: true,
            vision: true,
        }
    }

    fn chat_with_system<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let result = self
                .call_api(system_prompt, message, model, temperature)
                .await?;
            Self::extract_text(&result)
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
            let result = self
                .call_api(system_prompt, message, model, temperature)
                .await?;
            let text = Self::extract_text(&result)?;
            let mut provider_response = if let Some(usage) = result.usage_metadata {
                ProviderResponse::with_usage(
                    text,
                    usage.prompt_token_count,
                    usage.candidates_token_count,
                )
            } else {
                ProviderResponse::text_only(text)
            };
            if let Some(model_version) = result.model_version {
                provider_response = provider_response.with_model(model_version);
            }
            Ok(provider_response)
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
            let request = Self::build_tools_request(system_prompt, messages, tools, temperature);
            let result = self.call_api_with_request(model, &request).await?;

            let candidate = result
                .candidates
                .as_ref()
                .and_then(|candidates| candidates.first())
                .ok_or_else(|| anyhow::anyhow!("No response from Gemini"))?;

            let content_blocks = Self::parse_content_blocks(&candidate.content.parts);
            let text = {
                let mut out = String::new();
                for block in &content_blocks {
                    if let ContentBlock::Text { text: t } = block {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(t);
                    }
                }
                out
            };

            let mut provider_response = if let Some(usage) = result.usage_metadata {
                ProviderResponse::with_usage(
                    text,
                    usage.prompt_token_count,
                    usage.candidates_token_count,
                )
            } else {
                ProviderResponse::text_only(text)
            };

            provider_response.content_blocks = content_blocks;
            provider_response.stop_reason = Some(Self::map_stop_reason(candidate));

            if let Some(model_version) = result.model_version {
                provider_response = provider_response.with_model(model_version);
            }

            Ok(provider_response)
        })
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
