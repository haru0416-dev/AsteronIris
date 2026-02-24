use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(in crate::llm) struct ChatRequest {
    pub(in crate::llm) model: String,
    pub(in crate::llm) messages: Vec<Message>,
    pub(in crate::llm) temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::llm) tools: Option<Vec<OpenAiTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::llm) stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::llm) stream_options: Option<StreamOptions>,
}

#[derive(Debug, Serialize)]
pub(in crate::llm) struct StreamOptions {
    pub(in crate::llm) include_usage: bool,
}

#[derive(Debug, Serialize)]
pub(in crate::llm) struct Message {
    pub(in crate::llm) role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::llm) content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::llm) tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(in crate::llm) tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(in crate::llm) enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::llm) enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrlContent },
}

#[derive(Debug, Serialize)]
pub(in crate::llm) struct ImageUrlContent {
    pub(in crate::llm) url: String,
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::llm) struct OpenAiTool {
    pub(in crate::llm) r#type: &'static str,
    pub(in crate::llm) function: OpenAiToolDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::llm) struct OpenAiToolDefinition {
    pub(in crate::llm) name: String,
    pub(in crate::llm) description: String,
    pub(in crate::llm) parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::llm) struct OpenAiToolCall {
    pub(in crate::llm) id: String,
    pub(in crate::llm) r#type: String,
    pub(in crate::llm) function: OpenAiToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::llm) struct OpenAiToolCallFunction {
    pub(in crate::llm) name: String,
    pub(in crate::llm) arguments: String,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ChatResponse {
    pub(in crate::llm) choices: Vec<Choice>,
    pub(in crate::llm) usage: Option<Usage>,
    pub(in crate::llm) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct Usage {
    pub(in crate::llm) prompt_tokens: u64,
    pub(in crate::llm) completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct Choice {
    pub(in crate::llm) message: ResponseMessage,
    pub(in crate::llm) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ResponseMessage {
    pub(in crate::llm) content: Option<String>,
    pub(in crate::llm) tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ChatCompletionChunk {
    pub(in crate::llm) model: Option<String>,
    pub(in crate::llm) choices: Vec<ChunkChoice>,
    pub(in crate::llm) usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ChunkChoice {
    pub(in crate::llm) delta: ChunkDelta,
    pub(in crate::llm) finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ChunkDelta {
    pub(in crate::llm) content: Option<String>,
    pub(in crate::llm) tool_calls: Option<Vec<ChunkToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ChunkToolCall {
    pub(in crate::llm) index: u32,
    pub(in crate::llm) id: Option<String>,
    pub(in crate::llm) function: Option<ChunkToolCallFunction>,
}

#[derive(Debug, Deserialize)]
pub(in crate::llm) struct ChunkToolCallFunction {
    pub(in crate::llm) name: Option<String>,
    pub(in crate::llm) arguments: Option<String>,
}
