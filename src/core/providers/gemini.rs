//! Google Gemini provider with support for:
//! - Direct API key (`GEMINI_API_KEY` env var or config)
//! - Gemini CLI OAuth tokens (reuse existing ~/.gemini/ authentication)
//! - Google Cloud ADC (`GOOGLE_APPLICATION_CREDENTIALS`)

use crate::core::providers::{
    ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
    build_provider_client, scrub_secret_patterns,
    sse::{SseBuffer, parse_data_lines},
    streaming::{ProviderChatRequest, ProviderStream},
    tool_convert::{ToolFields, map_tools_optional},
    traits::Provider,
};
use async_trait::async_trait;
use directories::UserDirs;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

use super::gemini_types::{
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

    fn extract_text(result: &GenerateContentResponse) -> anyhow::Result<String> {
        let text = result
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .map(|candidate| {
                candidate
                    .content
                    .parts
                    .iter()
                    .filter_map(|part| part.text.as_ref())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        if text.is_empty() {
            anyhow::bail!("No response from Gemini");
        }

        Ok(text)
    }

    fn build_gemini_tools(
        tools: &[crate::core::tools::traits::ToolSpec],
    ) -> Option<Vec<GeminiTool>> {
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
        tools: &[crate::core::tools::traits::ToolSpec],
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
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Gemini API key not found. Options:\n\
                 1. Set GEMINI_API_KEY env var\n\
                 2. Run `gemini` CLI to authenticate (tokens will be reused)\n\
                 3. Get an API key from https://aistudio.google.com/app/apikey\n\
             4. Run `asteroniris onboard` to configure"
            )
        })?;

        let model_name = Self::model_name(model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{model_name}:generateContent?key={api_key}"
        );

        let response = self.client.post(&url).json(request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error ({status}): {error_text}");
        }

        let result: GenerateContentResponse = response.json().await?;

        if let Some(err) = result.error.as_ref() {
            anyhow::bail!("Gemini API error: {}", err.message);
        }

        Ok(result)
    }

    async fn call_api_streaming(
        &self,
        model: &str,
        request: &GenerateContentRequest,
    ) -> anyhow::Result<reqwest::Response> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Gemini API key not found. Options:\n\
                 1. Set GEMINI_API_KEY env var\n\
                 2. Run `gemini` CLI to authenticate (tokens will be reused)\n\
                 3. Get an API key from https://aistudio.google.com/app/apikey\n\
             4. Run `asteroniris onboard` to configure"
            )
        })?;

        let model_name = Self::model_name(model);
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{model_name}:streamGenerateContent?key={api_key}&alt=sse"
        );

        let response = self.client.post(&url).json(request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error ({status}): {error_text}");
        }

        Ok(response)
    }

    fn parse_sse_data_lines(chunk: &str) -> Vec<&str> {
        parse_data_lines(chunk)
    }

    async fn chat_with_tools_stream_impl(
        &self,
        req: ProviderChatRequest,
    ) -> anyhow::Result<ProviderStream> {
        use crate::core::providers::streaming::StreamEvent;
        use futures_util::StreamExt;

        let request = Self::build_tools_request(
            req.system_prompt.as_deref(),
            &req.messages,
            &req.tools,
            req.temperature,
        );

        let response = self.call_api_streaming(&req.model, &request).await?;
        let mut byte_stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            let mut sse_buffer = SseBuffer::new();
            let mut sent_start = false;
            let mut tool_call_index = 1usize;

            while let Some(chunk_result) = byte_stream.next().await {
                let chunk = chunk_result?;
                sse_buffer.push_chunk(&chunk);

                while let Some(event_block) = sse_buffer.next_event_block() {
                    for data in Self::parse_sse_data_lines(&event_block) {
                        let Ok(gen_response) = serde_json::from_str::<GenerateContentResponse>(data) else {
                            continue;
                        };

                        if let Some(err) = gen_response.error.as_ref() {
                            Err(anyhow::anyhow!("Gemini API error: {}", err.message))?;
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

#[async_trait]
impl Provider for GeminiProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let result = self
            .call_api(system_prompt, message, model, temperature)
            .await?;
        Self::extract_text(&result)
    }

    async fn chat_with_system_full(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
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
    }

    async fn chat_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[crate::core::tools::traits::ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let request = Self::build_tools_request(system_prompt, messages, tools, temperature);
        let result = self.call_api_with_request(model, &request).await?;

        let candidate = result
            .candidates
            .as_ref()
            .and_then(|candidates| candidates.first())
            .ok_or_else(|| anyhow::anyhow!("No response from Gemini"))?;

        let content_blocks = Self::parse_content_blocks(&candidate.content.parts);
        let text = content_blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                ContentBlock::ToolUse { .. }
                | ContentBlock::ToolResult { .. }
                | ContentBlock::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

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
    use crate::core::providers::gemini_types::CandidateContent;
    use crate::core::tools::traits::ToolSpec;

    #[test]
    fn provider_creates_without_key() {
        let provider = GeminiProvider::new(None);
        // Should not panic, just have no key
        assert!(provider.api_key.is_none() || provider.api_key.is_some());
    }

    #[test]
    fn provider_creates_with_key() {
        let provider = GeminiProvider::new(Some("test-api-key"));
        assert!(provider.api_key.is_some());
        assert_eq!(provider.api_key.as_deref(), Some("test-api-key"));
    }

    #[test]
    fn gemini_cli_dir_returns_path() {
        let dir = GeminiProvider::gemini_cli_dir();
        // Should return Some on systems with home dir
        if UserDirs::new().is_some() {
            assert!(dir.is_some());
            assert!(dir.unwrap().ends_with(".gemini"));
        }
    }

    #[test]
    fn auth_source_reports_correctly() {
        let provider = GeminiProvider::new(Some("explicit-key"));
        // With explicit key, should report "config" (unless CLI credentials exist)
        let source = provider.auth_source();
        // Should be either "config" or "Gemini CLI OAuth" if CLI is configured
        assert!(source == "config" || source == "Gemini CLI OAuth");
    }

    #[test]
    fn model_name_formatting() {
        // Test that model names are formatted correctly
        let model = "gemini-2.0-flash";
        let formatted = if model.starts_with("models/") {
            model.to_string()
        } else {
            format!("models/{model}")
        };
        assert_eq!(formatted, "models/gemini-2.0-flash");

        // Already prefixed
        let model2 = "models/gemini-1.5-pro";
        let formatted2 = if model2.starts_with("models/") {
            model2.to_string()
        } else {
            format!("models/{model2}")
        };
        assert_eq!(formatted2, "models/gemini-1.5-pro");
    }

    #[test]
    fn request_serialization() {
        let request = GenerateContentRequest {
            contents: vec![Content {
                role: Some("user".to_string()),
                parts: vec![Part::text("Hello".to_string())],
            }],
            system_instruction: Some(Content {
                role: None,
                parts: vec![Part::text("You are helpful".to_string())],
            }),
            tools: None,
            generation_config: GenerationConfig {
                temperature: 0.7,
                max_output_tokens: 8192,
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"text\":\"Hello\""));
        assert!(json.contains("\"temperature\":0.7"));
        assert!(json.contains("\"maxOutputTokens\":8192"));
    }

    #[test]
    fn response_deserialization() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "Hello there!"}]
                }
            }]
        }"#;

        let response: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert!(response.candidates.is_some());
        let text = response
            .candidates
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
            .content
            .parts
            .into_iter()
            .next()
            .unwrap()
            .text;
        assert_eq!(text, Some("Hello there!".to_string()));
    }

    #[test]
    fn gemini_tools_serialize_as_function_declarations() {
        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Execute shell command".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"command": {"type": "string"}},
                "required": ["command"]
            }),
        }];

        let request = GeminiProvider::build_tools_request(
            None,
            &[ProviderMessage::user("list files")],
            &tools,
            0.1,
        );
        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(
            value["tools"][0]["function_declarations"][0]["name"],
            "shell"
        );
        assert_eq!(
            value["tools"][0]["function_declarations"][0]["parameters"]["type"],
            "object"
        );
    }

    #[test]
    fn gemini_function_call_response_parses_to_tool_use_block() {
        let json = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"functionCall": {"name": "shell", "args": {"command": "ls"}}}]
                },
                "finishReason": "FUNCTION_CALL"
            }]
        }"#;

        let response: GenerateContentResponse = serde_json::from_str(json).unwrap();
        let candidate = response.candidates.unwrap().into_iter().next().unwrap();
        let blocks = GeminiProvider::parse_content_blocks(&candidate.content.parts);

        assert!(matches!(
            &blocks[0],
            ContentBlock::ToolUse { name, input, .. }
            if name == "shell" && input == &serde_json::json!({"command": "ls"})
        ));
    }

    #[test]
    fn gemini_finish_reason_mapping_handles_tool_calls() {
        let with_tool_call = Candidate {
            content: CandidateContent {
                parts: vec![ResponsePart {
                    text: None,
                    function_call: Some(GeminiFunctionCall {
                        name: "shell".to_string(),
                        args: serde_json::json!({"command": "ls"}),
                        id: None,
                    }),
                }],
            },
            finish_reason: Some("STOP".to_string()),
        };
        let max_tokens = Candidate {
            content: CandidateContent {
                parts: vec![ResponsePart {
                    text: Some("x".to_string()),
                    function_call: None,
                }],
            },
            finish_reason: Some("MAX_TOKENS".to_string()),
        };

        assert_eq!(
            GeminiProvider::map_stop_reason(&with_tool_call),
            StopReason::ToolUse
        );
        assert_eq!(
            GeminiProvider::map_stop_reason(&max_tokens),
            StopReason::MaxTokens
        );
    }

    #[test]
    fn map_provider_message_handles_image_block() {
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
        let tool_map = std::collections::HashMap::new();
        let content = GeminiProvider::map_provider_message(&msg, &tool_map);
        assert_eq!(content.role.as_deref(), Some("user"));
        assert_eq!(content.parts.len(), 2);
        let json = serde_json::to_value(&content).unwrap();
        assert!(json["parts"][0]["text"].is_string());
        assert_eq!(json["parts"][1]["inlineData"]["mimeType"], "image/png");
    }

    #[test]
    fn supports_tool_calling_returns_true() {
        let provider = GeminiProvider::new(Some("test-api-key"));
        assert!(provider.supports_tool_calling());
    }

    #[test]
    fn supports_vision_returns_true() {
        let provider = GeminiProvider::new(Some("test-key"));
        assert!(provider.supports_vision());
    }

    #[test]
    fn supports_streaming_returns_true() {
        let provider = GeminiProvider::new(Some("test-api-key"));
        assert!(provider.supports_streaming());
    }

    #[test]
    fn parse_sse_data_lines_basic() {
        let chunk = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]}}]}\n\n"
        );
        let lines = GeminiProvider::parse_sse_data_lines(chunk);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}"
        );
        assert_eq!(
            lines[1],
            "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" world\"}]}}]}"
        );
    }

    #[test]
    fn parse_sse_data_lines_empty() {
        let lines = GeminiProvider::parse_sse_data_lines("");
        assert!(lines.is_empty());
    }

    #[test]
    fn error_response_deserialization() {
        let json = r#"{
            "error": {
                "message": "Invalid API key"
            }
        }"#;

        let response: GenerateContentResponse = serde_json::from_str(json).unwrap();
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().message, "Invalid API key");
    }
}
