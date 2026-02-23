use crate::core::providers::response::{ContentBlock, ProviderResponse, StopReason};
use crate::core::providers::scrub::scrub_secret_patterns;
use anyhow::Result;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

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

pub trait StreamSink: Send + Sync {
    fn on_event<'a>(
        &'a self,
        event: &'a StreamEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

#[derive(Debug, Default)]
pub struct NullStreamSink;

impl StreamSink for NullStreamSink {
    fn on_event<'a>(
        &'a self,
        _event: &'a StreamEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async {})
    }
}

pub struct ChannelStreamSink {
    sender: mpsc::Sender<String>,
    buffer: Mutex<String>,
    flush_threshold: usize,
}

impl ChannelStreamSink {
    pub fn new(sender: mpsc::Sender<String>, flush_threshold: usize) -> Self {
        Self {
            sender,
            buffer: Mutex::new(String::new()),
            flush_threshold: flush_threshold.max(1),
        }
    }

    fn at_flush_boundary(text: &str) -> bool {
        text.ends_with(char::is_whitespace)
            || text.ends_with('.')
            || text.ends_with('!')
            || text.ends_with('?')
    }

    async fn flush_buffer(&self) {
        let mut guard = self.buffer.lock().await;
        if guard.is_empty() {
            return;
        }

        let payload = std::mem::take(&mut *guard);
        let _ = self.sender.send(payload).await;
    }
}

impl StreamSink for ChannelStreamSink {
    fn on_event<'a>(
        &'a self,
        event: &'a StreamEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            match event {
                StreamEvent::TextDelta { text } => {
                    let mut guard = self.buffer.lock().await;
                    guard.push_str(text);
                    let flush_now = guard.len() >= self.flush_threshold
                        && (Self::at_flush_boundary(&guard)
                            || guard.len() >= self.flush_threshold * 2);
                    if !flush_now {
                        return;
                    }
                    let payload = std::mem::take(&mut *guard);
                    drop(guard);
                    let _ = self.sender.send(payload).await;
                }
                StreamEvent::Done { .. } => {
                    self.flush_buffer().await;
                }
                StreamEvent::ResponseStart { .. }
                | StreamEvent::ToolCallDelta { .. }
                | StreamEvent::ToolCallComplete { .. } => {}
            }
        })
    }
}

pub struct CliStreamSink {
    writer: Arc<dyn Fn(&str) + Send + Sync>,
}

impl CliStreamSink {
    pub fn new() -> Self {
        Self {
            writer: Arc::new(|text| {
                eprint!("{text}");
            }),
        }
    }

    #[cfg(test)]
    fn with_writer(writer: Arc<dyn Fn(&str) + Send + Sync>) -> Self {
        Self { writer }
    }
}

impl Default for CliStreamSink {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSink for CliStreamSink {
    fn on_event<'a>(
        &'a self,
        event: &'a StreamEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if let StreamEvent::TextDelta { text } = event {
                (self.writer)(text);
            }
        })
    }
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
        let mut combined = std::mem::take(&mut self.carry);
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
mod tests;
