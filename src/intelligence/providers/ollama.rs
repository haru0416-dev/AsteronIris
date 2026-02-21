use crate::intelligence::providers::{
    ProviderMessage, ProviderResponse, build_provider_client_with_timeout,
    fallback_tools::{augment_system_prompt_with_tools, build_fallback_response},
    traits::{Provider, messages_to_text},
};
use crate::intelligence::tools::traits::ToolSpec;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OllamaProvider {
    base_url: String,
    client: Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    options: Options,
}

#[derive(Debug, Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct Options {
    temperature: f64,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ResponseMessage,
    prompt_eval_count: Option<u64>,
    eval_count: Option<u64>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

impl OllamaProvider {
    pub fn new(base_url: Option<&str>) -> Self {
        Self {
            base_url: base_url
                .unwrap_or("http://localhost:11434")
                .trim_end_matches('/')
                .to_string(),
            client: build_provider_client_with_timeout(300),
        }
    }

    fn build_request(
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> ChatRequest {
        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system",
                content: sys.to_string(),
            });
        }

        messages.push(Message {
            role: "user",
            content: message.to_string(),
        });

        ChatRequest {
            model: model.to_string(),
            messages,
            stream: false,
            options: Options { temperature },
        }
    }

    async fn call_api(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let request = Self::build_request(system_prompt, message, model, temperature);
        let url = format!("{}/api/chat", self.base_url);

        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let err = super::api_error("Ollama", response).await;
            anyhow::bail!("{err}. Is Ollama running? (brew install ollama && ollama serve)");
        }

        response.json().await.map_err(anyhow::Error::msg)
    }

    fn prepare_fallback_input(
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
    ) -> (String, String) {
        let augmented_prompt = augment_system_prompt_with_tools(system_prompt.unwrap_or(""), tools);
        let text = messages_to_text(messages);
        (augmented_prompt, text)
    }
}

#[async_trait]
impl Provider for OllamaProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let chat_response = self
            .call_api(system_prompt, message, model, temperature)
            .await?;
        Ok(chat_response.message.content)
    }

    async fn chat_with_system_full(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let chat_response = self
            .call_api(system_prompt, message, model, temperature)
            .await?;
        let text = chat_response.message.content;
        let mut provider_response =
            match (chat_response.prompt_eval_count, chat_response.eval_count) {
                (Some(input_tokens), Some(output_tokens)) => {
                    ProviderResponse::with_usage(text, input_tokens, output_tokens)
                }
                _ => ProviderResponse::text_only(text),
            };
        if let Some(api_model) = chat_response.model {
            provider_response = provider_response.with_model(api_model);
        }
        Ok(provider_response)
    }

    async fn chat_with_tools(
        &self,
        system_prompt: Option<&str>,
        messages: &[ProviderMessage],
        tools: &[ToolSpec],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderResponse> {
        let (augmented_prompt, text) = Self::prepare_fallback_input(system_prompt, messages, tools);
        let response = self
            .chat_with_system_full(Some(&augmented_prompt), &text, model, temperature)
            .await?;
        Ok(build_fallback_response(response, tools))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intelligence::providers::{ContentBlock, MessageRole, Provider};

    #[test]
    fn default_url() {
        let p = OllamaProvider::new(None);
        assert_eq!(p.base_url, "http://localhost:11434");
    }

    #[test]
    fn custom_url_trailing_slash() {
        let p = OllamaProvider::new(Some("http://192.168.1.100:11434/"));
        assert_eq!(p.base_url, "http://192.168.1.100:11434");
    }

    #[test]
    fn custom_url_no_trailing_slash() {
        let p = OllamaProvider::new(Some("http://myserver:11434"));
        assert_eq!(p.base_url, "http://myserver:11434");
    }

    #[test]
    fn empty_url_uses_empty() {
        let p = OllamaProvider::new(Some(""));
        assert_eq!(p.base_url, "");
    }

    #[test]
    fn request_serializes_with_system() {
        let req = ChatRequest {
            model: "llama3".to_string(),
            messages: vec![
                Message {
                    role: "system",
                    content: "You are AsteronIris".to_string(),
                },
                Message {
                    role: "user",
                    content: "hello".to_string(),
                },
            ],
            stream: false,
            options: Options { temperature: 0.7 },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"stream\":false"));
        assert!(json.contains("llama3"));
        assert!(json.contains("system"));
        assert!(json.contains("\"temperature\":0.7"));
    }

    #[test]
    fn request_serializes_without_system() {
        let req = ChatRequest {
            model: "mistral".to_string(),
            messages: vec![Message {
                role: "user",
                content: "test".to_string(),
            }],
            stream: false,
            options: Options { temperature: 0.0 },
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("\"role\":\"system\""));
        assert!(json.contains("mistral"));
    }

    #[test]
    fn response_deserializes() {
        let json = r#"{"message":{"role":"assistant","content":"Hello from Ollama!"}}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.message.content, "Hello from Ollama!");
    }

    #[test]
    fn response_with_empty_content() {
        let json = r#"{"message":{"role":"assistant","content":""}}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.message.content.is_empty());
    }

    #[test]
    fn response_with_multiline() {
        let json = r#"{"message":{"role":"assistant","content":"line1\nline2\nline3"}}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.message.content.contains("line1"));
    }

    #[test]
    fn fallback_input_includes_tool_schema_in_augmented_prompt() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "list files".to_string(),
            }],
        }];
        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Execute shell command".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                }
            }),
        }];

        let (prompt, text) =
            OllamaProvider::prepare_fallback_input(Some("You are helpful"), &messages, &tools);

        assert!(prompt.contains("## Available Tools"));
        assert!(prompt.contains("shell: Execute shell command"));
        assert!(text.contains("User: list files"));
    }

    #[test]
    fn supports_tool_calling_returns_false() {
        let provider = OllamaProvider::new(None);
        assert!(!provider.supports_tool_calling());
    }
}
