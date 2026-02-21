use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(super) struct ChatRequest {
    pub(super) model: String,
    pub(super) max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) system: Option<String>,
    pub(super) messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<Vec<AnthropicToolDef>>,
    pub(super) temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stream: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct Message {
    pub(super) role: &'static str,
    pub(super) content: MessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum MessageContent {
    Text(String),
    Blocks(Vec<InputContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum InputContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    Image {
        source: AnthropicImageSource,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum AnthropicImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

#[derive(Debug, Serialize)]
pub(super) struct AnthropicToolDef {
    pub(super) name: String,
    pub(super) description: String,
    pub(super) input_schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatResponse {
    pub(super) content: Vec<ResponseContentBlock>,
    pub(super) stop_reason: Option<String>,
    pub(super) usage: Option<Usage>,
    pub(super) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Usage {
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponseContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamMessageStart {
    pub(super) message: StreamMessageInfo,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamMessageInfo {
    pub(super) model: Option<String>,
    pub(super) usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamContentBlockStart {
    pub(super) index: u32,
    pub(super) content_block: StreamContentBlockType,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamContentBlockType {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamContentBlockDelta {
    pub(super) index: u32,
    pub(super) delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamMessageDelta {
    pub(super) delta: StreamMessageDeltaBody,
    pub(super) usage: Option<StreamDeltaUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamMessageDeltaBody {
    pub(super) stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamDeltaUsage {
    pub(super) output_tokens: u64,
}
