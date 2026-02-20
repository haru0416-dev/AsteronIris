use crate::providers::response::{ContentBlock, ProviderMessage, ProviderResponse, StopReason};
use crate::providers::scrub::scrub_secret_patterns;
use crate::tools::traits::ToolSpec;
use anyhow::Result;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send + 'static>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    ResponseStart {
        model: Option<String>,
    },
    TextDelta {
        text: String,
    },
    ToolCallDelta {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        input_json_delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Done {
        stop_reason: Option<StopReason>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderChatRequest {
    pub system_prompt: Option<String>,
    pub messages: Vec<ProviderMessage>,
    pub tools: Vec<ToolSpec>,
    pub model: String,
    pub temperature: f64,
}

pub struct StreamCollector {
    text: String,
    content_blocks: Vec<ContentBlock>,
    tool_call_builders: Vec<ToolCallBuilder>,
    stop_reason: Option<StopReason>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    model: Option<String>,
}

struct ToolCallBuilder {
    id: String,
    name: String,
    input_json: String,
}

impl StreamCollector {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            content_blocks: Vec::new(),
            tool_call_builders: Vec::new(),
            stop_reason: None,
            input_tokens: None,
            output_tokens: None,
            model: None,
        }
    }

    pub fn feed(&mut self, event: &StreamEvent) {
        match event {
            StreamEvent::ResponseStart { model } => {
                self.model.clone_from(model);
            }
            StreamEvent::TextDelta { text } => {
                self.text.push_str(text);
            }
            StreamEvent::ToolCallDelta {
                index,
                id,
                name,
                input_json_delta,
            } => {
                if let Ok(builder_index) = usize::try_from(*index) {
                    while self.tool_call_builders.len() <= builder_index {
                        self.tool_call_builders.push(ToolCallBuilder {
                            id: String::new(),
                            name: String::new(),
                            input_json: String::new(),
                        });
                    }

                    let builder = &mut self.tool_call_builders[builder_index];
                    if let Some(call_id) = id {
                        builder.id.clone_from(call_id);
                    }
                    if let Some(call_name) = name {
                        builder.name.clone_from(call_name);
                    }
                    builder.input_json.push_str(input_json_delta);
                } else {
                    tracing::warn!(
                        index,
                        "Skipping tool call delta due to non-convertible index"
                    );
                }
            }
            StreamEvent::ToolCallComplete { id, name, input } => {
                self.content_blocks.push(ContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
            }
            StreamEvent::Done {
                stop_reason,
                input_tokens,
                output_tokens,
            } => {
                self.stop_reason = *stop_reason;
                self.input_tokens = *input_tokens;
                self.output_tokens = *output_tokens;
            }
        }
    }

    pub fn finish(mut self) -> ProviderResponse {
        for builder in self.tool_call_builders {
            if builder.id.is_empty() || builder.name.is_empty() {
                if !builder.input_json.trim().is_empty() {
                    tracing::warn!("Skipping incomplete streamed tool call (missing id or name)");
                }
                continue;
            }

            match serde_json::from_str::<serde_json::Value>(&builder.input_json) {
                Ok(input) => {
                    self.content_blocks.push(ContentBlock::ToolUse {
                        id: builder.id,
                        name: builder.name,
                        input,
                    });
                }
                Err(error) => {
                    tracing::warn!(
                        tool_id = builder.id,
                        tool_name = builder.name,
                        "Skipping malformed streamed tool call JSON: {error}"
                    );
                }
            }
        }

        if !self.text.is_empty() {
            self.content_blocks.insert(
                0,
                ContentBlock::Text {
                    text: self.text.clone(),
                },
            );
        }

        ProviderResponse {
            text: self.text,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            model: self.model,
            content_blocks: self.content_blocks,
            stop_reason: self.stop_reason,
        }
    }
}

pub fn resp_to_events(resp: ProviderResponse) -> Vec<Result<StreamEvent>> {
    let ProviderResponse {
        text,
        input_tokens,
        output_tokens,
        model,
        content_blocks,
        stop_reason,
    } = resp;

    let mut events = vec![Ok(StreamEvent::ResponseStart { model })];
    if !text.is_empty() {
        events.push(Ok(StreamEvent::TextDelta { text }));
    }
    for block in content_blocks {
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                events.push(Ok(StreamEvent::ToolCallComplete { id, name, input }));
            }
            ContentBlock::Text { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Image { .. } => {}
        }
    }
    events.push(Ok(StreamEvent::Done {
        stop_reason,
        input_tokens,
        output_tokens,
    }));
    events
}

pub struct StreamingSecretScrubber {
    carry: String,
    window: usize,
}

impl StreamingSecretScrubber {
    pub fn new(window: usize) -> Self {
        Self {
            carry: String::new(),
            window: window.max(64),
        }
    }

    pub fn scrub_delta(&mut self, delta: &str) -> String {
        let mut combined = self.carry.clone();
        combined.push_str(delta);

        let scrubbed = scrub_secret_patterns(&combined).into_owned();
        if scrubbed.len() > self.window {
            let mut split_at = scrubbed.len() - self.window;
            while split_at > 0 && !scrubbed.is_char_boundary(split_at) {
                split_at -= 1;
            }

            let emitted = scrubbed[..split_at].to_string();
            self.carry = scrubbed[split_at..].to_string();
            emitted
        } else {
            self.carry = scrubbed;
            String::new()
        }
    }

    pub fn finish(self) -> String {
        scrub_secret_patterns(&self.carry).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderChatRequest, StreamCollector, StreamEvent, StreamingSecretScrubber};
    use crate::providers::response::{ContentBlock, ProviderMessage, ProviderResponse, StopReason};
    use crate::providers::streaming::resp_to_events;

    #[test]
    fn stream_event_text_delta_debug() {
        let event = StreamEvent::TextDelta {
            text: "hello".to_string(),
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("TextDelta"));
        assert!(debug.contains("hello"));
    }

    #[test]
    fn provider_chat_request_clone() {
        let request = ProviderChatRequest {
            system_prompt: Some("system".to_string()),
            messages: vec![ProviderMessage::user("hello")],
            tools: vec![],
            model: "test-model".to_string(),
            temperature: 0.7,
        };

        let clone = request.clone();
        assert_eq!(clone.system_prompt, request.system_prompt);
        assert_eq!(clone.messages.len(), request.messages.len());
        assert_eq!(clone.model, request.model);
        assert_eq!(clone.temperature, request.temperature);
    }

    #[test]
    fn collector_text_only() {
        let mut collector = StreamCollector::new();
        collector.feed(&StreamEvent::ResponseStart {
            model: Some("model".to_string()),
        });
        collector.feed(&StreamEvent::TextDelta {
            text: "hello world".to_string(),
        });
        collector.feed(&StreamEvent::Done {
            stop_reason: Some(StopReason::EndTurn),
            input_tokens: Some(10),
            output_tokens: Some(2),
        });

        let response = collector.finish();
        assert_eq!(response.text, "hello world");
        assert_eq!(response.model, Some("model".to_string()));
    }

    #[test]
    fn collector_tool_call_complete() {
        let mut collector = StreamCollector::new();
        collector.feed(&StreamEvent::ResponseStart {
            model: Some("model".to_string()),
        });
        collector.feed(&StreamEvent::ToolCallComplete {
            id: "call-1".to_string(),
            name: "shell".to_string(),
            input: serde_json::json!({"command": "ls"}),
        });
        collector.feed(&StreamEvent::Done {
            stop_reason: Some(StopReason::ToolUse),
            input_tokens: None,
            output_tokens: None,
        });

        let response = collector.finish();
        assert_eq!(response.content_blocks.len(), 1);
        assert!(matches!(
            response.content_blocks[0],
            ContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn collector_tool_call_delta_assembly() {
        let mut collector = StreamCollector::new();
        collector.feed(&StreamEvent::ResponseStart { model: None });
        collector.feed(&StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call-1".to_string()),
            name: Some("shell".to_string()),
            input_json_delta: "{\"co".to_string(),
        });
        collector.feed(&StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            input_json_delta: "mmand\"".to_string(),
        });
        collector.feed(&StreamEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: None,
            input_json_delta: ": \"ls\"}".to_string(),
        });
        collector.feed(&StreamEvent::Done {
            stop_reason: Some(StopReason::ToolUse),
            input_tokens: None,
            output_tokens: None,
        });

        let response = collector.finish();
        assert_eq!(response.content_blocks.len(), 1);
        match &response.content_blocks[0] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call-1");
                assert_eq!(name, "shell");
                assert_eq!(input, &serde_json::json!({"command": "ls"}));
            }
            _ => panic!("expected tool use block"),
        }
    }

    #[test]
    fn collector_mixed_text_and_tools() {
        let mut collector = StreamCollector::new();
        collector.feed(&StreamEvent::ResponseStart { model: None });
        collector.feed(&StreamEvent::TextDelta {
            text: "running".to_string(),
        });
        collector.feed(&StreamEvent::ToolCallComplete {
            id: "call-1".to_string(),
            name: "shell".to_string(),
            input: serde_json::json!({"command": "pwd"}),
        });
        collector.feed(&StreamEvent::Done {
            stop_reason: Some(StopReason::ToolUse),
            input_tokens: Some(1),
            output_tokens: Some(1),
        });

        let response = collector.finish();
        assert_eq!(response.text, "running");
        assert_eq!(response.content_blocks.len(), 2);
        assert!(matches!(
            response.content_blocks[0],
            ContentBlock::Text { .. }
        ));
        assert!(matches!(
            response.content_blocks[1],
            ContentBlock::ToolUse { .. }
        ));
    }

    #[test]
    fn collector_invalid_tool_json_skipped() {
        let mut collector = StreamCollector::new();
        collector.feed(&StreamEvent::ResponseStart { model: None });
        collector.feed(&StreamEvent::ToolCallDelta {
            index: 0,
            id: Some("call-1".to_string()),
            name: Some("shell".to_string()),
            input_json_delta: "{\"command\": }".to_string(),
        });
        collector.feed(&StreamEvent::Done {
            stop_reason: Some(StopReason::Error),
            input_tokens: None,
            output_tokens: None,
        });

        let response = collector.finish();
        assert!(response.content_blocks.is_empty());
    }

    #[test]
    fn resp_to_events_roundtrip() {
        let original = ProviderResponse {
            text: "hello".to_string(),
            input_tokens: Some(10),
            output_tokens: Some(4),
            model: Some("model".to_string()),
            content_blocks: vec![
                ContentBlock::Text {
                    text: "hello".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "call-1".to_string(),
                    name: "shell".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
        };

        let events = resp_to_events(original.clone());
        let mut collector = StreamCollector::new();
        for event in events {
            collector.feed(&event.expect("event should be ok"));
        }

        let reconstructed = collector.finish();
        assert_eq!(reconstructed.text, original.text);
        assert_eq!(reconstructed.model, original.model);
        assert_eq!(reconstructed.stop_reason, original.stop_reason);
        assert_eq!(reconstructed.input_tokens, original.input_tokens);
        assert_eq!(reconstructed.output_tokens, original.output_tokens);
        assert_eq!(
            reconstructed.content_blocks.len(),
            original.content_blocks.len()
        );
        match (
            &reconstructed.content_blocks[0],
            &original.content_blocks[0],
        ) {
            (ContentBlock::Text { text: left }, ContentBlock::Text { text: right }) => {
                assert_eq!(left, right);
            }
            _ => panic!("expected first content block to be text"),
        }
        match (
            &reconstructed.content_blocks[1],
            &original.content_blocks[1],
        ) {
            (
                ContentBlock::ToolUse {
                    id: left_id,
                    name: left_name,
                    input: left_input,
                },
                ContentBlock::ToolUse {
                    id: right_id,
                    name: right_name,
                    input: right_input,
                },
            ) => {
                assert_eq!(left_id, right_id);
                assert_eq!(left_name, right_name);
                assert_eq!(left_input, right_input);
            }
            _ => panic!("expected second content block to be tool_use"),
        }
    }

    #[test]
    fn scrubber_passes_clean_text() {
        let mut scrubber = StreamingSecretScrubber::new(64);
        let first = scrubber.scrub_delta("hello world");
        let rest = scrubber.finish();
        assert_eq!(format!("{first}{rest}"), "hello world");
    }

    #[test]
    fn scrubber_redacts_secret() {
        let mut scrubber = StreamingSecretScrubber::new(64);
        let mut output = scrubber.scrub_delta("key is sk-abc123def456");
        output.push_str(&scrubber.finish());
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("sk-abc123def456"));
    }

    #[test]
    fn scrubber_finish_flushes_carry() {
        let mut scrubber = StreamingSecretScrubber::new(64);
        let prefix = scrubber.scrub_delta("partial");
        let suffix = scrubber.finish();
        assert_eq!(format!("{prefix}{suffix}"), "partial");
    }

    #[test]
    fn scrubber_split_across_chunks() {
        let mut scrubber = StreamingSecretScrubber::new(64);
        let mut output = scrubber.scrub_delta("key is sk-");
        output.push_str(&scrubber.scrub_delta("abc123def456 ok"));
        output.push_str(&scrubber.finish());

        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("sk-abc123def456"));
    }
}
