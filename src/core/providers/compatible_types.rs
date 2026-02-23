use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(super) struct ChatRequest {
    pub(super) model: String,
    pub(super) messages: Vec<Message>,
    pub(super) temperature: f64,
}

#[derive(Debug, Serialize)]
pub(super) struct Message {
    pub(super) role: &'static str,
    pub(super) content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatResponse {
    pub(super) choices: Vec<Choice>,
    pub(super) usage: Option<ChatUsage>,
    pub(super) model: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChatUsage {
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct Choice {
    pub(super) message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponseMessage {
    pub(super) content: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ResponsesRequest {
    pub(super) model: String,
    pub(super) input: Vec<ResponsesInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) store: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) stream: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct ResponsesInput {
    pub(super) role: &'static str,
    pub(super) content: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponsesResponse {
    #[serde(default)]
    pub(super) output: Vec<ResponsesOutput>,
    #[serde(default)]
    pub(super) output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponsesOutput {
    #[serde(default)]
    pub(super) content: Vec<ResponsesContent>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ResponsesContent {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    pub(super) text: Option<String>,
}

fn first_nonempty(text: Option<&str>) -> Option<String> {
    text.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(super) fn extract_responses_text(response: &ResponsesResponse) -> Option<String> {
    if let Some(text) = first_nonempty(response.output_text.as_deref()) {
        return Some(text);
    }

    for item in &response.output {
        for content in &item.content {
            if content.kind.as_deref() == Some("output_text")
                && let Some(text) = first_nonempty(content.text.as_deref())
            {
                return Some(text);
            }
        }
    }

    for item in &response.output {
        for content in &item.content {
            if let Some(text) = first_nonempty(content.text.as_deref()) {
                return Some(text);
            }
        }
    }

    None
}

pub(super) fn extract_responses_sse_text(body: &str) -> Option<String> {
    let mut output_text = String::new();
    let mut snapshot: Option<String> = None;

    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let payload = data.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            continue;
        };

        if let Some(text) = value
            .pointer("/response/output_text")
            .and_then(Value::as_str)
            .and_then(|v| first_nonempty(Some(v)))
        {
            snapshot = Some(text);
        }

        if let Some(text) = value
            .pointer("/output_text")
            .and_then(Value::as_str)
            .and_then(|v| first_nonempty(Some(v)))
        {
            snapshot = Some(text);
        }

        if let Some(delta) = value.pointer("/delta").and_then(Value::as_str) {
            output_text.push_str(delta);
        }
    }

    if !output_text.trim().is_empty() {
        return Some(output_text.trim().to_string());
    }

    snapshot
}

pub(super) fn extract_chat_text(
    response: &ChatResponse,
    provider_name: &str,
) -> anyhow::Result<String> {
    response
        .choices
        .first()
        .map(|choice| choice.message.content.clone())
        .ok_or_else(|| anyhow::anyhow!("No response from {provider_name}"))
}
