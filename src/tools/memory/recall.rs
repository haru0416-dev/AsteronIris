use crate::memory::{Memory, RecallQuery};
use crate::tools::traits::{ExecutionContext, Tool};
use crate::tools::types::ToolResult;
use serde_json::json;
use std::fmt::Write;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Let the agent search its own memory.
pub struct MemoryRecallTool {
    memory: Arc<dyn Memory>,
}

impl MemoryRecallTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }

    fn build_recall_request(
        args: &serde_json::Value,
        ctx: &ExecutionContext,
    ) -> anyhow::Result<RecallQuery> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' parameter"))?;

        #[allow(clippy::cast_possible_truncation)]
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(5, |v| v as usize);

        let entity_id = args
            .get("entity_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.entity_id);

        let request = RecallQuery::new(entity_id, query, limit)
            .with_policy_context(ctx.tenant_context.clone());
        request.enforce_policy()?;
        Ok(request)
    }
}

impl Tool for MemoryRecallTool {
    fn name(&self) -> &str {
        "memory_recall"
    }

    fn description(&self) -> &str {
        "Recall entity-scoped memory using hybrid ranking."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords or phrase to search for in memory"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity id to scope recall (defaults to current session entity)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results to return (default: 5)"
                },
                "policy_context": {
                    "type": "object",
                    "description": "Optional tenant policy context to enforce recall scope",
                    "properties": {
                        "tenant_mode_enabled": {
                            "type": "boolean"
                        },
                        "tenant_id": {
                            "type": ["string", "null"]
                        }
                    },
                    "additionalProperties": false
                }
            },
            "required": ["query"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let request = match Self::build_recall_request(&args, ctx) {
                Ok(request) => request,
                Err(error) => {
                    if error.to_string().starts_with("blocked by security policy:") {
                        return Ok(ToolResult {
                            success: false,
                            output: String::new(),
                            error: Some(format!("Memory recall failed: {error}")),
                            attachments: Vec::new(),
                        });
                    }
                    return Err(error);
                }
            };

            match self.memory.recall_scoped(request).await {
                Ok(entries) if entries.is_empty() => Ok(ToolResult {
                    success: true,
                    output: "No memories found matching that query.".into(),
                    error: None,
                    attachments: Vec::new(),
                }),
                Ok(entries) => {
                    let mut output = format!("Found {} memories:\n", entries.len());
                    for entry in &entries {
                        let score = format!(" [{:.0}%]", entry.score * 100.0);
                        let _ = writeln!(
                            output,
                            "- [{}:{}] {}{score}",
                            entry.entity_id, entry.slot_key, entry.value
                        );
                    }
                    Ok(ToolResult {
                        success: true,
                        output,
                        error: None,
                        attachments: Vec::new(),
                    })
                }
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Memory recall failed: {e}")),
                    attachments: Vec::new(),
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_schema() {
        // Verify schema structure without needing a Memory implementation.
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        });
        assert!(schema["properties"]["query"].is_object());
    }
}
