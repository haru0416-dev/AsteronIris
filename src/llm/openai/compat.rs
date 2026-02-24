use super::types::{
    ChatCompletionChunk, ChatRequest, ChatResponse, ContentPart, ImageUrlContent, Message,
    MessageContent, OpenAiTool, OpenAiToolCall, OpenAiToolCallFunction, OpenAiToolDefinition,
    StreamOptions, Usage,
};
use crate::llm::types::{
    ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
};
use crate::llm::{
    scrub_secret_patterns,
    sse::{SseBuffer, parse_data_lines_without_done},
    streaming::{ProviderStream, StreamEvent},
    tool_convert::{ToolFields, map_tools_optional},
};
use crate::tools::ToolSpec;
use anyhow::Context;
use futures_util::StreamExt;
use serde_json::Value;

pub(in crate::llm) fn build_request(
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

pub(in crate::llm) fn build_text_message(role: &'static str, content: String) -> Message {
    Message {
        role,
        content: Some(MessageContent::Text(content)),
        tool_call_id: None,
        tool_calls: None,
    }
}

pub(in crate::llm) fn map_provider_message(provider_message: &ProviderMessage) -> Vec<Message> {
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
                    messages.push(build_text_message("user", content));
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
                messages.push(build_text_message("system", content));
            }
        }
    }

    messages.extend(tool_messages);
    messages
}

pub(in crate::llm) fn build_openai_tools(tools: &[ToolSpec]) -> Option<Vec<OpenAiTool>> {
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

fn build_messages(system_prompt: Option<&str>, messages: &[ProviderMessage]) -> Vec<Message> {
    let mut openai_messages = Vec::new();

    if let Some(sys) = system_prompt {
        openai_messages.push(build_text_message(
            "system",
            scrub_secret_patterns(sys).into_owned(),
        ));
    }

    for provider_message in messages {
        openai_messages.extend(map_provider_message(provider_message));
    }

    openai_messages
}

pub(in crate::llm) fn build_tools_request(
    system_prompt: Option<&str>,
    messages: &[ProviderMessage],
    tools: &[ToolSpec],
    model: &str,
    temperature: f64,
) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: build_messages(system_prompt, messages),
        temperature,
        tools: build_openai_tools(tools),
        stream: None,
        stream_options: None,
    }
}

pub(in crate::llm) fn build_stream_request(
    system_prompt: Option<&str>,
    messages: &[ProviderMessage],
    tools: &[ToolSpec],
    model: &str,
    temperature: f64,
) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: build_messages(system_prompt, messages),
        temperature,
        tools: build_openai_tools(tools),
        stream: Some(true),
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
    }
}

pub(in crate::llm) fn extract_text(
    chat_response: &ChatResponse,
    provider_name: &str,
) -> anyhow::Result<String> {
    chat_response
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .ok_or_else(|| anyhow::anyhow!("No response from {provider_name}"))
}

pub(in crate::llm) fn map_finish_reason(finish_reason: Option<&str>) -> StopReason {
    match finish_reason {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        Some(_) | None => StopReason::Error,
    }
}

pub(in crate::llm) fn parse_tool_calls(
    tool_calls: Option<Vec<OpenAiToolCall>>,
    provider_name: &str,
) -> anyhow::Result<Vec<ContentBlock>> {
    tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|tool_call| {
            let input: Value =
                serde_json::from_str(&tool_call.function.arguments).with_context(|| {
                    format!(
                        "{provider_name} tool call arguments were not valid JSON for {}",
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

pub(in crate::llm) struct ChatCompletionsEndpoint<'a> {
    pub(in crate::llm) provider_name: &'a str,
    pub(in crate::llm) url: &'a str,
    pub(in crate::llm) missing_api_key_message: &'a str,
    pub(in crate::llm) extra_headers: &'a [(&'a str, &'a str)],
}

pub(in crate::llm) async fn send_chat_completions_raw(
    client: &reqwest::Client,
    cached_auth_header: Option<&String>,
    request: &ChatRequest,
    endpoint: ChatCompletionsEndpoint<'_>,
) -> anyhow::Result<reqwest::Response> {
    let auth_header = cached_auth_header
        .ok_or_else(|| anyhow::anyhow!("{}", endpoint.missing_api_key_message))?;

    let mut request_builder = client
        .post(endpoint.url)
        .header("Authorization", auth_header)
        .json(request);

    for (name, value) in endpoint.extra_headers {
        request_builder = request_builder.header(*name, *value);
    }

    let response = request_builder
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("{} request failed: {error}", endpoint.provider_name))?;

    if !response.status().is_success() {
        return Err(crate::llm::scrub::api_error(endpoint.provider_name, response).await);
    }

    Ok(response)
}

pub(in crate::llm) async fn send_chat_completions_json(
    client: &reqwest::Client,
    cached_auth_header: Option<&String>,
    request: &ChatRequest,
    endpoint: ChatCompletionsEndpoint<'_>,
) -> anyhow::Result<ChatResponse> {
    let provider_name = endpoint.provider_name;
    let response = send_chat_completions_raw(client, cached_auth_header, request, endpoint).await?;

    response
        .json()
        .await
        .map_err(|error| anyhow::anyhow!("{provider_name} response JSON decode failed: {error}"))
}

fn provider_response_with_usage(text: String, usage: Option<&Usage>) -> ProviderResponse {
    if let Some(usage) = usage {
        ProviderResponse::with_usage(text, usage.prompt_tokens, usage.completion_tokens)
    } else {
        ProviderResponse::text_only(text)
    }
}

pub(in crate::llm) fn build_text_provider_response(
    chat_response: ChatResponse,
    provider_name: &str,
) -> anyhow::Result<ProviderResponse> {
    let text = extract_text(&chat_response, provider_name)?;
    let mut provider_response = provider_response_with_usage(text, chat_response.usage.as_ref());

    if let Some(api_model) = chat_response.model {
        provider_response = provider_response.with_model(api_model);
    }

    Ok(provider_response)
}

pub(in crate::llm) fn build_tool_provider_response(
    chat_response: ChatResponse,
    provider_name: &str,
) -> anyhow::Result<ProviderResponse> {
    let choice = chat_response
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("No response from {provider_name}"))?;

    let text = choice.message.content.clone().unwrap_or_default();
    let scrubbed_text = scrub_secret_patterns(&text).into_owned();
    let mut content_blocks = parse_tool_calls(choice.message.tool_calls.clone(), provider_name)?;

    if !scrubbed_text.is_empty() {
        content_blocks.insert(
            0,
            ContentBlock::Text {
                text: scrubbed_text.clone(),
            },
        );
    }

    let mut provider_response =
        provider_response_with_usage(scrubbed_text, chat_response.usage.as_ref());
    provider_response.content_blocks = content_blocks;
    provider_response.stop_reason = Some(map_finish_reason(choice.finish_reason.as_deref()));

    if let Some(api_model) = chat_response.model {
        provider_response = provider_response.with_model(api_model);
    }

    Ok(provider_response)
}

pub(in crate::llm) fn sse_response_to_provider_stream(
    response: reqwest::Response,
) -> ProviderStream {
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
                            let stop = map_finish_reason(Some(finish));
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

    Box::pin(stream)
}
