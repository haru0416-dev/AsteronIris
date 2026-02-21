use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(super) struct ChatRequest {
    pub(super) model: String,
    pub(super) messages: Vec<Message>,
    pub(super) temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<Vec<OpenAiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
pub(super) struct StreamOptions {
    pub(super) include_usage: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct Message {
    pub(super) role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlContent },
}

#[derive(Debug, Serialize)]
pub(super) struct ImageUrlContent {
    pub(super) url: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct OpenAiTool {
    pub(super) r#type: &'static str,
    pub(super) function: OpenAiToolDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct OpenAiToolDefinition {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OpenAiToolCall {
    pub(super) id: String,
    pub(super) r#type: String,
    pub(super) function: OpenAiToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OpenAiToolCallFunction {
    pub(super) name: String,
    pub(super) arguments: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatResponse {
    pub(super) choices: Vec<Choice>,
    pub(super) usage: Option<Usage>,
    pub(super) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Usage {
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct Choice {
    pub(super) message: ResponseMessage,
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponseMessage {
    pub(super) content: Option<String>,
    pub(super) tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatCompletionChunk {
    pub(super) model: Option<String>,
    pub(super) choices: Vec<ChunkChoice>,
    pub(super) usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChunkChoice {
    pub(super) delta: ChunkDelta,
    pub(super) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChunkDelta {
    pub(super) content: Option<String>,
    pub(super) tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChunkToolCall {
    pub(super) index: u32,
    pub(super) id: Option<String>,
    pub(super) function: Option<ChunkToolCallFunction>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChunkToolCallFunction {
    pub(super) name: Option<String>,
    pub(super) arguments: Option<String>,
}
