//! Example: Implementing a custom Provider for AsteronIris
//!
//! Demonstrates the dyn-safe async trait pattern used throughout the codebase.
//! Each async method returns `Pin<Box<dyn Future<Output = R> + Send + '_>>`.
//!
//! Run: `cargo run --example custom_provider`

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

// ── Minimal Provider trait (mirrors src/core/providers/traits.rs) ────

pub trait Provider: Send + Sync {
    fn chat<'a>(
        &'a self,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

    fn supports_streaming(&self) -> bool;
}

// ── Example: Ollama local provider ──────────────────────────────────

pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new("http://localhost:11434")
    }
}

impl Provider for OllamaProvider {
    fn chat<'a>(
        &'a self,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!("{}/api/generate", self.base_url);

            let body = serde_json::json!({
                "model": model,
                "prompt": message,
                "temperature": temperature,
                "stream": false,
            });

            let resp = self
                .client
                .post(&url)
                .json(&body)
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?;

            resp["response"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("No response field in Ollama reply"))
        })
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}

// ── Demo ─────────────────────────────────────────────────────────────

fn main() {
    // Providers are stored as `Arc<dyn Provider>` — the trait is dyn-safe.
    let provider: Box<dyn Provider> = Box::new(OllamaProvider::default());
    println!(
        "Provider streaming support: {}",
        provider.supports_streaming()
    );
    println!("Register your provider in src/core/providers/factory.rs");
}
