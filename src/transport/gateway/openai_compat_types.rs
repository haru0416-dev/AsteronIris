use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<RequestMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    #[allow(dead_code)]
    pub max_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct RequestMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletion {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Option<CompletionUsage>,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChoiceMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChoiceMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        ChatCompletion, ChatCompletionChunk, ChatCompletionRequest, Choice, ChoiceMessage,
        ChunkChoice, ChunkDelta,
    };

    #[test]
    fn deserializes_chat_completion_request() {
        let payload = serde_json::json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ],
            "temperature": 0.5,
            "stream": true,
            "max_tokens": 128
        });

        let parsed: ChatCompletionRequest = serde_json::from_value(payload).unwrap();
        assert_eq!(parsed.model, "gpt-4o-mini");
        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.temperature, Some(0.5));
        assert_eq!(parsed.stream, Some(true));
        assert_eq!(parsed.max_tokens, Some(128));
    }

    #[test]
    fn serializes_chat_completion() {
        let completion = ChatCompletion {
            id: "chatcmpl-1".to_string(),
            object: "chat.completion".to_string(),
            created: 1,
            model: "gpt-test".to_string(),
            choices: vec![Choice {
                index: 0,
                message: ChoiceMessage {
                    role: "assistant".to_string(),
                    content: "hello".to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: None,
        };

        let value = serde_json::to_value(completion).unwrap();
        assert_eq!(value["object"], "chat.completion");
        assert_eq!(value["choices"][0]["message"]["content"], "hello");
    }

    #[test]
    fn serializes_chat_completion_chunk() {
        let chunk = ChatCompletionChunk {
            id: "chatcmpl-1".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 1,
            model: "gpt-test".to_string(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant".to_string()),
                    content: Some("hello".to_string()),
                },
                finish_reason: None,
            }],
        };

        let value = serde_json::to_value(chunk).unwrap();
        assert_eq!(value["object"], "chat.completion.chunk");
        assert_eq!(value["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(value["choices"][0]["delta"]["content"], "hello");
    }
}
