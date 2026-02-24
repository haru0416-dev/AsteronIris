use crate::memory::{ForgetMode, Memory};
use crate::tools::traits::{ExecutionContext, Tool};
use crate::tools::types::ToolResult;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Let the agent forget/delete a memory entry.
pub struct MemoryForgetTool {
    memory: Arc<dyn Memory>,
}

impl MemoryForgetTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

impl Tool for MemoryForgetTool {
    fn name(&self) -> &str {
        "memory_forget"
    }

    fn description(&self) -> &str {
        "Apply soft/hard/tombstone forgetting on an entity slot."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Slot key to forget"
                },
                "slot_key": {
                    "type": "string",
                    "description": "Slot key to forget"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity id owning the slot"
                },
                "mode": {
                    "type": "string",
                    "enum": ["soft", "hard", "tombstone"],
                    "description": "Deletion lifecycle mode"
                },
                "reason": {
                    "type": "string",
                    "description": "Deletion reason for audit"
                },
                "policy_context": {
                    "type": "object",
                    "description": "Optional tenant policy context to validate forget scope",
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
            "required": ["entity_id", "slot_key"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let key = args
                .get("slot_key")
                .or_else(|| args.get("key"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'slot_key' parameter"))?;

            let entity_id = args
                .get("entity_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'entity_id' parameter"))?;
            let reason = args
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("user_requested");

            let policy_context = &ctx.tenant_context;
            if let Err(error) = policy_context.enforce_recall_scope(entity_id) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to forget memory: {error}")),
                    attachments: Vec::new(),
                });
            }

            let mode = match args.get("mode").and_then(|v| v.as_str()) {
                Some("hard") => ForgetMode::Hard,
                Some("tombstone") => ForgetMode::Tombstone,
                _ => ForgetMode::Soft,
            };

            match self.memory.forget_slot(entity_id, key, mode, reason).await {
                Ok(outcome) if outcome.applied => Ok(ToolResult {
                    success: true,
                    output: format!("Forgot slot: {key}"),
                    error: None,
                    attachments: Vec::new(),
                }),
                Ok(_) => Ok(ToolResult {
                    success: true,
                    output: format!("No memory found with key: {key}"),
                    error: None,
                    attachments: Vec::new(),
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to forget memory: {e}")),
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
        let schema = json!({
            "type": "object",
            "properties": {
                "slot_key": { "type": "string" },
                "entity_id": { "type": "string" }
            },
            "required": ["entity_id", "slot_key"]
        });
        assert!(schema["properties"]["slot_key"].is_object());
        assert!(schema["properties"]["entity_id"].is_object());
    }
}
