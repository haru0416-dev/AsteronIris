use crate::providers::{ProviderResponse, traits::Provider};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct OpenRouterProvider {
    api_key: Option<String>,
    client: Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

impl OpenRouterProvider {
    pub fn new(api_key: Option<&str>) -> Self {
        Self {
            api_key: api_key.map(ToString::to_string),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
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
                role: "system".to_string(),
                content: sys.to_string(),
            });
        }

        messages.push(Message {
            role: "user".to_string(),
            content: message.to_string(),
        });

        ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
        }
    }

    fn extract_text(chat_response: &ChatResponse) -> anyhow::Result<String> {
        chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("No response from OpenRouter"))
    }

    async fn call_api(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let api_key = self.api_key.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "OpenRouter API key not set. Run `asteroniris onboard` or set OPENROUTER_API_KEY env var."
            )
        })?;

        let request = Self::build_request(system_prompt, message, model, temperature);

        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {api_key}"))
            .header(
                "HTTP-Referer",
                "https://github.com/haru0416-dev/AsteronIris",
            )
            .header("X-Title", "AsteronIris")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(super::api_error("OpenRouter", response).await);
        }

        response.json().await.map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    async fn warmup(&self) -> anyhow::Result<()> {
        // Hit a lightweight endpoint to establish TLS + HTTP/2 connection pool.
        // This prevents the first real chat request from timing out on cold start.
        if let Some(api_key) = self.api_key.as_ref() {
            self.client
                .get("https://openrouter.ai/api/v1/auth/key")
                .header("Authorization", format!("Bearer {api_key}"))
                .send()
                .await?
                .error_for_status()?;
        }
        Ok(())
    }

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
        Self::extract_text(&chat_response)
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
        let text = Self::extract_text(&chat_response)?;
        let mut provider_response = if let Some(usage) = chat_response.usage {
            ProviderResponse::with_usage(text, usage.prompt_tokens, usage.completion_tokens)
        } else {
            ProviderResponse::text_only(text)
        };
        if let Some(api_model) = chat_response.model {
            provider_response = provider_response.with_model(api_model);
        }
        Ok(provider_response)
    }
}
