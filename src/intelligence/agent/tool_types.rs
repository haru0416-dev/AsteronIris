use crate::intelligence::providers::response::ProviderMessage;
use crate::intelligence::providers::streaming::StreamSink;
use crate::intelligence::tools::registry::ToolRegistry;
use crate::intelligence::tools::traits::{OutputAttachment, ToolResult, ToolSpec};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(crate) const TOOL_LOOP_HARD_CAP: u32 = 25;

pub struct ToolLoop {
    pub(crate) registry: Arc<ToolRegistry>,
    pub(crate) max_iterations: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: ToolResult,
    pub iteration: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopStopReason {
    Completed,
    MaxIterations,
    Error(String),
    ApprovalDenied,
    RateLimited,
}

pub struct ToolLoopResult {
    pub final_text: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub attachments: Vec<OutputAttachment>,
    pub iterations: u32,
    pub tokens_used: Option<u64>,
    pub stop_reason: LoopStopReason,
}

pub(crate) struct ChatOnceInput<'a> {
    pub(crate) system_prompt: Option<&'a str>,
    pub(crate) messages: &'a [ProviderMessage],
    pub(crate) tool_specs: &'a [ToolSpec],
    pub(crate) model: &'a str,
    pub(crate) temperature: f64,
    pub(crate) stream_sink: Option<&'a dyn StreamSink>,
}

pub(crate) struct ToolUseExecutionOutcome {
    pub(crate) had_tool_use: bool,
    pub(crate) stop_reason: Option<LoopStopReason>,
}
