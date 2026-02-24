//! Example: Implementing a custom Tool for AsteronIris
//!
//! Tools are the agent's hands — they let it interact with the world.
//! The trait uses `Pin<Box<dyn Future<...> + Send>>` for dyn-safety,
//! so tools are collected as `Vec<Box<dyn Tool>>`.
//!
//! Run: `cargo run --example custom_tool`

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use serde_json::{Value, json};

// ── Minimal types (mirrors src/core/tools/traits.rs) ────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

// ── Minimal Tool trait ──────────────────────────────────────────────

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;

    fn execute<'a>(
        &'a self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>>;
}

// ── Example: A tool that fetches a URL ──────────────────────────────

pub struct HttpGetTool;

impl Tool for HttpGetTool {
    fn name(&self) -> &str {
        "http_get"
    }

    fn description(&self) -> &str {
        "Fetch a URL and return the HTTP status code and content length"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" }
            },
            "required": ["url"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: Value,
    ) -> Pin<Box<dyn Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let url = args["url"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'url' parameter"))?;

            match reqwest::get(url).await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let len = resp.content_length().unwrap_or(0);
                    Ok(ToolResult {
                        success: status < 400,
                        output: format!("HTTP {status} — {len} bytes"),
                        error: None,
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Request failed: {e}")),
                }),
            }
        })
    }
}

// ── Demo ─────────────────────────────────────────────────────────────

fn main() {
    // Tools are stored as `Vec<Box<dyn Tool>>` — the trait is dyn-safe.
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(HttpGetTool)];
    println!("Registered {} tool(s): {}", tools.len(), tools[0].name());
    println!("Register your tool in src/core/tools/ default_tools()");
}
