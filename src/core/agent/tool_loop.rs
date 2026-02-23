use crate::core::providers::response::{
    ContentBlock, MessageRole, ProviderMessage, ProviderResponse, StopReason,
};
use crate::core::providers::streaming::{ProviderChatRequest, StreamCollector};
use crate::core::providers::traits::Provider;
use crate::core::tools::middleware::ExecutionContext;
use crate::core::tools::registry::ToolRegistry;
use crate::core::tools::traits::{OutputAttachment, ToolResult, ToolSpec};
use futures_util::StreamExt;
use std::sync::Arc;

use super::tool_execution::{
    build_result, classify_execute_error, format_tool_result_content, is_action_limit_message,
};
use super::tool_types::{ChatOnceInput, TOOL_LOOP_HARD_CAP, ToolUseExecutionOutcome};

pub use super::tool_execution::augment_prompt_with_trust_boundary;
pub use super::tool_types::{
    LoopStopReason, ToolCallRecord, ToolLoop, ToolLoopResult, ToolLoopRunParams,
};

impl ToolLoop {
    pub fn new(registry: Arc<ToolRegistry>, max_iterations: u32) -> Self {
        Self {
            registry,
            max_iterations: max_iterations.min(TOOL_LOOP_HARD_CAP),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub async fn run(&self, params: ToolLoopRunParams<'_>) -> anyhow::Result<ToolLoopResult> {
        let ToolLoopRunParams {
            provider,
            system_prompt,
            user_message,
            image_content,
            model,
            temperature,
            ctx,
            stream_sink,
            conversation_history,
        } = params;
        let tool_specs: Vec<ToolSpec> = self.registry.specs_for_context(ctx);
        let prompt = augment_prompt_with_trust_boundary(system_prompt, !tool_specs.is_empty());
        let initial_message = if image_content.is_empty() {
            ProviderMessage::user(user_message)
        } else {
            let mut content = vec![ContentBlock::Text {
                text: user_message.to_string(),
            }];
            content.extend(image_content.iter().cloned());
            ProviderMessage {
                role: MessageRole::User,
                content,
            }
        };
        let mut messages = Vec::with_capacity(conversation_history.len() + 1);
        messages.extend_from_slice(conversation_history);
        messages.push(initial_message);
        let mut tool_calls = Vec::new();
        let mut attachments = Vec::new();
        let mut iterations = 0_u32;
        let mut token_sum = 0_u64;
        let mut saw_tokens = false;

        loop {
            iterations = iterations.saturating_add(1);
            if iterations > self.max_iterations {
                return Ok(build_result(
                    &messages,
                    tool_calls,
                    attachments,
                    iterations.saturating_sub(1),
                    token_sum,
                    saw_tokens,
                    LoopStopReason::MaxIterations,
                ));
            }

            let mut turn_ctx = ctx.clone();
            turn_ctx.turn_number = iterations;

            let response = match self
                .chat_once(
                    provider,
                    ChatOnceInput {
                        system_prompt: Some(prompt.as_str()),
                        messages: &messages,
                        tool_specs: &tool_specs,
                        model,
                        temperature,
                        stream_sink: stream_sink.as_deref(),
                    },
                )
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    return Ok(build_result(
                        &messages,
                        tool_calls,
                        attachments,
                        iterations,
                        token_sum,
                        saw_tokens,
                        LoopStopReason::Error(error.to_string()),
                    ));
                }
            };

            if let Some(tokens) = response.total_tokens() {
                token_sum = token_sum.saturating_add(tokens);
                saw_tokens = true;
            }

            messages.push(response.to_assistant_message());

            if matches!(response.stop_reason, Some(StopReason::ToolUse)) || response.has_tool_use()
            {
                let outcome = self
                    .execute_tool_uses(
                        &response,
                        &turn_ctx,
                        iterations,
                        &mut messages,
                        &mut tool_calls,
                        &mut attachments,
                    )
                    .await;
                if let Some(reason) = outcome.stop_reason {
                    return Ok(build_result(
                        &messages,
                        tool_calls,
                        attachments,
                        iterations,
                        token_sum,
                        saw_tokens,
                        reason,
                    ));
                }
                if outcome.had_tool_use {
                    continue;
                }
            }

            return Ok(build_result(
                &messages,
                tool_calls,
                attachments,
                iterations,
                token_sum,
                saw_tokens,
                LoopStopReason::Completed,
            ));
        }
    }

    async fn chat_once(
        &self,
        provider: &dyn Provider,
        input: ChatOnceInput<'_>,
    ) -> anyhow::Result<ProviderResponse> {
        if provider.supports_streaming() {
            let req = ProviderChatRequest {
                system_prompt: input.system_prompt.map(String::from),
                messages: input.messages.to_vec(),
                tools: input.tool_specs.to_vec(),
                model: input.model.to_string(),
                temperature: input.temperature,
            };
            let mut stream = provider.chat_with_tools_stream(req).await?;
            let mut collector = StreamCollector::new();
            while let Some(event_result) = stream.next().await {
                let event = event_result?;
                if let Some(sink) = input.stream_sink {
                    sink.on_event(&event).await;
                }
                collector.feed(&event);
            }
            Ok(collector.finish())
        } else {
            provider
                .chat_with_tools(
                    input.system_prompt,
                    input.messages,
                    input.tool_specs,
                    input.model,
                    input.temperature,
                )
                .await
        }
    }

    async fn execute_tool_uses(
        &self,
        response: &ProviderResponse,
        ctx: &ExecutionContext,
        iteration: u32,
        messages: &mut Vec<ProviderMessage>,
        tool_calls: &mut Vec<ToolCallRecord>,
        attachments: &mut Vec<OutputAttachment>,
    ) -> ToolUseExecutionOutcome {
        let mut had_tool_use = false;

        for block in response.tool_use_blocks() {
            if let ContentBlock::ToolUse { id, name, input } = block {
                had_tool_use = true;
                let tool_result = match self.registry.execute(name, input.clone(), ctx).await {
                    Ok(result) => result,
                    Err(error) => {
                        if let Some(stop_reason) = classify_execute_error(&error.to_string()) {
                            return ToolUseExecutionOutcome {
                                had_tool_use,
                                stop_reason: Some(stop_reason),
                            };
                        }
                        ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(error.to_string()),
                            attachments: Vec::new(),
                        }
                    }
                };

                if tool_result
                    .error
                    .as_deref()
                    .is_some_and(is_action_limit_message)
                {
                    return ToolUseExecutionOutcome {
                        had_tool_use,
                        stop_reason: Some(LoopStopReason::RateLimited),
                    };
                }

                let tool_result_content = format_tool_result_content(&tool_result);
                attachments.extend(tool_result.attachments.iter().cloned());
                tool_calls.push(ToolCallRecord {
                    tool_name: name.clone(),
                    args: input.clone(),
                    result: tool_result.clone(),
                    iteration,
                });
                messages.push(ProviderMessage::tool_result(
                    id.clone(),
                    tool_result_content,
                    !tool_result.success,
                ));
            }
        }

        ToolUseExecutionOutcome {
            had_tool_use,
            stop_reason: None,
        }
    }

    #[cfg(test)]
    fn max_iterations(&self) -> u32 {
        self.max_iterations
    }
}

#[cfg(test)]
mod tests;
