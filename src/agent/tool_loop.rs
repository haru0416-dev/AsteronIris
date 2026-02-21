use crate::providers::response::{
    ContentBlock, MessageRole, ProviderMessage, ProviderResponse, StopReason,
};
use crate::providers::streaming::{ProviderChatRequest, StreamCollector, StreamSink};
use crate::providers::traits::Provider;
use crate::tools::middleware::ExecutionContext;
use crate::tools::registry::ToolRegistry;
use crate::tools::traits::{OutputAttachment, ToolResult, ToolSpec};
use futures_util::StreamExt;
use std::sync::Arc;

use super::tool_execution::{
    build_result, classify_execute_error, format_tool_result_content, is_action_limit_message,
};
use super::tool_types::{ChatOnceInput, TOOL_LOOP_HARD_CAP, ToolUseExecutionOutcome};

pub use super::tool_execution::augment_prompt_with_trust_boundary;
pub use super::tool_types::{LoopStopReason, ToolCallRecord, ToolLoop, ToolLoopResult};

impl ToolLoop {
    pub fn new(registry: Arc<ToolRegistry>, max_iterations: u32) -> Self {
        Self {
            registry,
            max_iterations: max_iterations.min(TOOL_LOOP_HARD_CAP),
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn run(
        &self,
        provider: &dyn Provider,
        system_prompt: &str,
        user_message: &str,
        image_content: &[ContentBlock],
        model: &str,
        temperature: f64,
        ctx: &ExecutionContext,
        stream_sink: Option<Arc<dyn StreamSink>>,
    ) -> anyhow::Result<ToolLoopResult> {
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
        let mut messages = vec![initial_message];
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
mod tests {
    use super::*;
    use crate::providers::response::{ProviderResponse, StopReason};
    use crate::providers::streaming::{ProviderChatRequest, StreamEvent, StreamSink};
    use crate::security::SecurityPolicy;
    use crate::tools::middleware::{MiddlewareDecision, ToolMiddleware};
    use crate::tools::traits::{OutputAttachment, Tool};
    use async_trait::async_trait;
    use futures_util::stream;
    use serde_json::{Value, json};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct EchoTool;

    #[derive(Debug)]
    struct AttachmentTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo_tool"
        }

        fn description(&self) -> &str {
            "Echo tool"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            args: Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                success: true,
                output: args.to_string(),
                error: None,

                attachments: Vec::new(),
            })
        }
    }

    #[async_trait]
    impl Tool for AttachmentTool {
        fn name(&self) -> &str {
            "attachment_tool"
        }

        fn description(&self) -> &str {
            "Attachment tool"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            args: Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            let index = args.get("index").and_then(Value::as_u64).unwrap_or(0);
            Ok(ToolResult {
                success: true,
                output: format!("attachment {index}"),
                error: None,
                attachments: vec![OutputAttachment::from_path(
                    "image/png",
                    format!("/tmp/generated-{index}.png"),
                    Some(format!("generated-{index}.png")),
                )],
            })
        }
    }

    #[derive(Debug)]
    struct CountingMiddleware {
        count: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl ToolMiddleware for CountingMiddleware {
        async fn before_execute(
            &self,
            _tool_name: &str,
            _args: &Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<MiddlewareDecision> {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(MiddlewareDecision::Continue)
        }

        async fn after_execute(
            &self,
            _tool_name: &str,
            _result: &mut ToolResult,
            _ctx: &ExecutionContext,
        ) {
        }
    }

    #[derive(Debug)]
    struct RateLimitMiddleware;

    #[async_trait]
    impl ToolMiddleware for RateLimitMiddleware {
        async fn before_execute(
            &self,
            _tool_name: &str,
            _args: &Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<MiddlewareDecision> {
            Ok(MiddlewareDecision::Block(
                "blocked by security policy: entity action limit exceeded for 'test'".to_string(),
            ))
        }

        async fn after_execute(
            &self,
            _tool_name: &str,
            _result: &mut ToolResult,
            _ctx: &ExecutionContext,
        ) {
        }
    }

    struct MockProvider {
        responses: Mutex<VecDeque<ProviderResponse>>,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }

        async fn chat_with_tools(
            &self,
            _system_prompt: Option<&str>,
            _messages: &[ProviderMessage],
            _tools: &[ToolSpec],
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ProviderResponse> {
            let mut guard = self
                .responses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(guard
                .pop_front()
                .unwrap_or_else(|| ProviderResponse::text_only("done".to_string())))
        }

        fn supports_tool_calling(&self) -> bool {
            true
        }
    }

    struct MockStreamingProvider {
        events: Vec<StreamEvent>,
    }

    #[async_trait]
    impl Provider for MockStreamingProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }

        async fn chat_with_tools(
            &self,
            _system_prompt: Option<&str>,
            _messages: &[ProviderMessage],
            _tools: &[ToolSpec],
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ProviderResponse> {
            Ok(ProviderResponse::text_only("fallback".to_string()))
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        async fn chat_with_tools_stream(
            &self,
            _req: ProviderChatRequest,
        ) -> anyhow::Result<crate::providers::streaming::ProviderStream> {
            let items = self
                .events
                .iter()
                .cloned()
                .map(Ok::<_, anyhow::Error>)
                .collect::<Vec<_>>();
            Ok(Box::pin(stream::iter(items)))
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        labels: Mutex<Vec<String>>,
        deltas: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl StreamSink for RecordingSink {
        async fn on_event(&self, event: &StreamEvent) {
            let label = match event {
                StreamEvent::ResponseStart { .. } => "response_start",
                StreamEvent::TextDelta { .. } => "text_delta",
                StreamEvent::ToolCallDelta { .. } => "tool_call_delta",
                StreamEvent::ToolCallComplete { .. } => "tool_call_complete",
                StreamEvent::Done { .. } => "done",
            };
            self.labels
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(label.to_string());
            if let StreamEvent::TextDelta { text } = event {
                self.deltas
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(text.clone());
            }
        }
    }

    fn test_ctx() -> ExecutionContext {
        let security = Arc::new(SecurityPolicy::default());
        ExecutionContext::test_default(security)
    }

    #[tokio::test]
    async fn loop_iterates_tool_use_then_end_turn() {
        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(EchoTool));
        let registry = Arc::new(registry);
        let loop_ = ToolLoop::new(registry, 10);

        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    text: String::new(),
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({"value": "ok"}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
                ProviderResponse {
                    text: "final answer".to_string(),
                    input_tokens: Some(8),
                    output_tokens: Some(4),
                    model: None,
                    content_blocks: vec![],
                    stop_reason: Some(StopReason::EndTurn),
                },
            ])),
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.stop_reason, LoopStopReason::Completed);
        assert_eq!(result.iterations, 2);
        assert_eq!(result.final_text, "final answer");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tokens_used, Some(27));
    }

    #[tokio::test]
    async fn loop_stops_at_max_iterations_when_tool_use_continues() {
        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(EchoTool));
        let loop_ = ToolLoop::new(Arc::new(registry), 2);
        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    text: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
                ProviderResponse {
                    text: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_2".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
                ProviderResponse {
                    text: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_3".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
            ])),
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.stop_reason, LoopStopReason::MaxIterations);
        assert_eq!(result.iterations, 2);
        assert_eq!(result.tool_calls.len(), 2);
    }

    #[test]
    fn hard_cap_is_enforced() {
        let registry = Arc::new(ToolRegistry::new(vec![]));
        let loop_ = ToolLoop::new(registry, 100);
        assert_eq!(loop_.max_iterations(), 25);
    }

    #[tokio::test]
    async fn loop_executes_tools_through_registry_middleware_chain() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = ToolRegistry::new(vec![Arc::new(CountingMiddleware {
            count: Arc::clone(&count),
        })]);
        registry.register(Box::new(EchoTool));
        let loop_ = ToolLoop::new(Arc::new(registry), 5);

        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    text: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({"a": 1}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
                ProviderResponse {
                    text: "done".to_string(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![],
                    stop_reason: Some(StopReason::EndTurn),
                },
            ])),
        };

        let _ = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rate_limit_error_stops_loop() {
        let mut registry = ToolRegistry::new(vec![Arc::new(RateLimitMiddleware)]);
        registry.register(Box::new(EchoTool));
        let loop_ = ToolLoop::new(Arc::new(registry), 5);
        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![ProviderResponse {
                text: String::new(),
                input_tokens: None,
                output_tokens: None,
                model: None,
                content_blocks: vec![ContentBlock::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "echo_tool".to_string(),
                    input: json!({}),
                }],
                stop_reason: Some(StopReason::ToolUse),
            }])),
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.stop_reason, LoopStopReason::RateLimited);
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn no_tools_registered_returns_single_turn_response() {
        let loop_ = ToolLoop::new(Arc::new(ToolRegistry::new(vec![])), 5);
        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![ProviderResponse {
                text: "plain response".to_string(),
                input_tokens: None,
                output_tokens: None,
                model: None,
                content_blocks: vec![],
                stop_reason: Some(StopReason::EndTurn),
            }])),
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.stop_reason, LoopStopReason::Completed);
        assert_eq!(result.iterations, 1);
        assert_eq!(result.final_text, "plain response");
        assert!(result.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn streaming_with_none_sink_preserves_behavior() {
        let loop_ = ToolLoop::new(Arc::new(ToolRegistry::new(vec![])), 5);
        let provider = MockStreamingProvider {
            events: vec![
                StreamEvent::ResponseStart { model: None },
                StreamEvent::TextDelta {
                    text: "hello".to_string(),
                },
                StreamEvent::TextDelta {
                    text: " world".to_string(),
                },
                StreamEvent::Done {
                    stop_reason: Some(StopReason::EndTurn),
                    input_tokens: Some(3),
                    output_tokens: Some(2),
                },
            ],
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.stop_reason, LoopStopReason::Completed);
        assert_eq!(result.final_text, "hello world");
        assert_eq!(result.tokens_used, Some(5));
        assert!(result.attachments.is_empty());
    }

    #[tokio::test]
    async fn loop_result_aggregates_attachments_across_tool_calls() {
        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(AttachmentTool));
        let loop_ = ToolLoop::new(Arc::new(registry), 10);
        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![
                ProviderResponse {
                    text: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "attachment_tool".to_string(),
                        input: json!({"index": 1}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
                ProviderResponse {
                    text: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![ContentBlock::ToolUse {
                        id: "toolu_2".to_string(),
                        name: "attachment_tool".to_string(),
                        input: json!({"index": 2}),
                    }],
                    stop_reason: Some(StopReason::ToolUse),
                },
                ProviderResponse {
                    text: "done".to_string(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![],
                    stop_reason: Some(StopReason::EndTurn),
                },
            ])),
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.attachments.len(), 2);
        assert_eq!(
            result.attachments[0].path.as_deref(),
            Some("/tmp/generated-1.png")
        );
        assert_eq!(
            result.attachments[1].path.as_deref(),
            Some("/tmp/generated-2.png")
        );
    }

    #[tokio::test]
    async fn loop_result_attachments_empty_without_tool_uses() {
        let loop_ = ToolLoop::new(Arc::new(ToolRegistry::new(vec![])), 5);
        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![ProviderResponse {
                text: "plain response".to_string(),
                input_tokens: None,
                output_tokens: None,
                model: None,
                content_blocks: vec![],
                stop_reason: Some(StopReason::EndTurn),
            }])),
        };

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                None,
            )
            .await
            .unwrap();

        assert!(result.attachments.is_empty());
    }

    #[tokio::test]
    async fn streaming_with_sink_receives_all_events_in_order() {
        let loop_ = ToolLoop::new(Arc::new(ToolRegistry::new(vec![])), 5);
        let provider = MockStreamingProvider {
            events: vec![
                StreamEvent::ResponseStart { model: None },
                StreamEvent::TextDelta {
                    text: "a".to_string(),
                },
                StreamEvent::TextDelta {
                    text: "b".to_string(),
                },
                StreamEvent::Done {
                    stop_reason: Some(StopReason::EndTurn),
                    input_tokens: None,
                    output_tokens: None,
                },
            ],
        };
        let sink = Arc::new(RecordingSink::default());

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                Some(Arc::clone(&sink) as Arc<dyn StreamSink>),
            )
            .await
            .unwrap();

        let labels = sink
            .labels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let deltas = sink
            .deltas
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        assert_eq!(result.final_text, "ab");
        assert_eq!(
            labels,
            vec!["response_start", "text_delta", "text_delta", "done"]
        );
        assert_eq!(deltas, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn streaming_sink_receives_non_text_events_without_breaking_collection() {
        let loop_ = ToolLoop::new(Arc::new(ToolRegistry::new(vec![])), 5);
        let provider = MockStreamingProvider {
            events: vec![
                StreamEvent::ResponseStart { model: None },
                StreamEvent::ToolCallDelta {
                    index: 0,
                    id: None,
                    name: None,
                    input_json_delta: "{\"command\":\"ls\"}".to_string(),
                },
                StreamEvent::Done {
                    stop_reason: Some(StopReason::ToolUse),
                    input_tokens: None,
                    output_tokens: None,
                },
            ],
        };
        let sink = Arc::new(RecordingSink::default());

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                Some(Arc::clone(&sink) as Arc<dyn StreamSink>),
            )
            .await
            .unwrap();

        let labels = sink
            .labels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();

        assert_eq!(labels, vec!["response_start", "tool_call_delta", "done"]);
        assert!(result.final_text.is_empty());
        assert_eq!(result.stop_reason, LoopStopReason::Completed);
        assert!(result.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn non_streaming_provider_does_not_emit_sink_events() {
        let loop_ = ToolLoop::new(Arc::new(ToolRegistry::new(vec![])), 5);
        let provider = MockProvider {
            responses: Mutex::new(VecDeque::from(vec![ProviderResponse {
                text: "plain response".to_string(),
                input_tokens: None,
                output_tokens: None,
                model: None,
                content_blocks: vec![],
                stop_reason: Some(StopReason::EndTurn),
            }])),
        };
        let sink = Arc::new(RecordingSink::default());

        let result = loop_
            .run(
                &provider,
                "system",
                "hello",
                &[],
                "test-model",
                0.2,
                &test_ctx(),
                Some(Arc::clone(&sink) as Arc<dyn StreamSink>),
            )
            .await
            .unwrap();

        let labels = sink
            .labels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(labels.is_empty());
        assert_eq!(result.final_text, "plain response");
    }

    #[test]
    fn trust_boundary_present_when_tools_available() {
        let prompt = augment_prompt_with_trust_boundary("base prompt", true);
        assert!(prompt.contains("## Tool Result Trust Policy"));
        assert!(prompt.contains("[[external-content:tool_result:*]]"));
    }

    #[test]
    fn trust_boundary_absent_when_no_tools_available() {
        let prompt = augment_prompt_with_trust_boundary("base prompt", false);
        assert_eq!(prompt, "base prompt");
    }
}
