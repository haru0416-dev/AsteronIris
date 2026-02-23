use super::*;
use crate::core::providers::response::{ProviderResponse, StopReason};
use crate::core::providers::streaming::{StreamEvent, StreamSink};
use crate::core::tools::middleware::{MiddlewareDecision, ToolMiddleware};
use crate::core::tools::traits::{OutputAttachment, Tool};
use crate::security::SecurityPolicy;
use futures_util::stream;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

#[derive(Debug)]
struct EchoTool;

#[derive(Debug)]
struct AttachmentTool;

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

    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            Ok(ToolResult {
                success: true,
                output: args.to_string(),
                error: None,

                attachments: Vec::new(),
            })
        })
    }
}

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

    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
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
        })
    }
}

#[derive(Debug)]
struct CountingMiddleware {
    count: Arc<std::sync::atomic::AtomicUsize>,
}

impl ToolMiddleware for CountingMiddleware {
    fn before_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(MiddlewareDecision::Continue)
        })
    }

    fn after_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {})
    }
}

#[derive(Debug)]
struct RateLimitMiddleware;

impl ToolMiddleware for RateLimitMiddleware {
    fn before_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move {
            Ok(MiddlewareDecision::Block(
                "blocked by security policy: entity action limit exceeded for 'test'".to_string(),
            ))
        })
    }

    fn after_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {})
    }
}

struct MockProvider {
    responses: Mutex<VecDeque<ProviderResponse>>,
}

impl Provider for MockProvider {
    fn chat_with_system<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        _message: &'a str,
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move { Ok(String::new()) })
    }

    fn chat_with_tools<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        _messages: &'a [ProviderMessage],
        _tools: &'a [ToolSpec],
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let mut guard = self
                .responses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(guard
                .pop_front()
                .unwrap_or_else(|| ProviderResponse::text_only("done".to_string())))
        })
    }

    fn supports_tool_calling(&self) -> bool {
        true
    }
}

struct MockStreamingProvider {
    events: Vec<StreamEvent>,
}

impl Provider for MockStreamingProvider {
    fn chat_with_system<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        _message: &'a str,
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move { Ok(String::new()) })
    }

    fn chat_with_tools<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        _messages: &'a [ProviderMessage],
        _tools: &'a [ToolSpec],
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move { Ok(ProviderResponse::text_only("fallback".to_string())) })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn chat_with_tools_stream<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        _messages: &'a [ProviderMessage],
        _tools: &'a [ToolSpec],
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<
        Box<
            dyn Future<Output = anyhow::Result<crate::core::providers::streaming::ProviderStream>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let items = self
                .events
                .iter()
                .cloned()
                .map(Ok::<_, anyhow::Error>)
                .collect::<Vec<_>>();
            Ok(Box::pin(stream::iter(items)) as crate::core::providers::streaming::ProviderStream)
        })
    }
}

#[derive(Default)]
struct RecordingSink {
    labels: Mutex<Vec<String>>,
    deltas: Mutex<Vec<String>>,
}

impl StreamSink for RecordingSink {
    fn on_event<'a>(
        &'a self,
        event: &'a StreamEvent,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
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
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: None,
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: Some(Arc::clone(&sink) as Arc<dyn StreamSink>),
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: Some(Arc::clone(&sink) as Arc<dyn StreamSink>),
            conversation_history: &[],
        })
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
        .run(ToolLoopRunParams {
            provider: &provider,
            system_prompt: "system",
            user_message: "hello",
            image_content: &[],
            model: "test-model",
            temperature: 0.2,
            ctx: &test_ctx(),
            stream_sink: Some(Arc::clone(&sink) as Arc<dyn StreamSink>),
            conversation_history: &[],
        })
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
