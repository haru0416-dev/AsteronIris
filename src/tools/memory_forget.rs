use super::traits::{Tool, ToolResult};
use crate::memory::{ForgetMode, Memory};
use crate::security::policy::TenantPolicyContext;
use crate::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Let the agent forget/delete a memory entry
pub struct MemoryForgetTool {
    memory: Arc<dyn Memory>,
}

impl MemoryForgetTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }

    fn parse_policy_context(args: &serde_json::Value) -> anyhow::Result<TenantPolicyContext> {
        let Some(raw_context) = args.get("policy_context") else {
            return Ok(TenantPolicyContext::disabled());
        };

        let Some(raw_context) = raw_context.as_object() else {
            anyhow::bail!("Invalid 'policy_context' parameter: expected object");
        };

        let tenant_mode_enabled = match raw_context.get("tenant_mode_enabled") {
            Some(value) => value.as_bool().ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid 'policy_context.tenant_mode_enabled' parameter: expected boolean"
                )
            })?,
            None => false,
        };

        let tenant_id = match raw_context.get("tenant_id") {
            Some(serde_json::Value::String(value)) => Some(value.clone()),
            Some(serde_json::Value::Null) | None => None,
            Some(_) => {
                anyhow::bail!(
                    "Invalid 'policy_context.tenant_id' parameter: expected string or null"
                )
            }
        };

        Ok(TenantPolicyContext {
            tenant_mode_enabled,
            tenant_id,
        })
    }
}

#[async_trait]
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

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
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

        let policy_context = Self::parse_policy_context(&args)?;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
    };
    use crate::tools::middleware::ExecutionContext;
    use tempfile::TempDir;

    fn test_mem() -> (TempDir, Arc<dyn Memory>) {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();
        (tmp, Arc::new(mem))
    }

    #[test]
    fn name_and_schema() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryForgetTool::new(mem);
        assert_eq!(tool.name(), "memory_forget");
        assert!(tool.parameters_schema()["properties"]["slot_key"].is_object());
    }

    #[tokio::test]
    async fn forget_existing() {
        let (_tmp, mem) = test_mem();
        mem.append_event(
            MemoryEventInput::new(
                "default",
                "temp",
                MemoryEventType::FactAdded,
                "temporary",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.6),
        )
        .await
        .unwrap();

        let tool = MemoryForgetTool::new(mem.clone());
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({"entity_id": "default", "slot_key": "temp", "mode": "hard"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("Forgot"));

        assert!(mem.resolve_slot("default", "temp").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn forget_nonexistent() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryForgetTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(json!({"entity_id": "default", "slot_key": "nope"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No memory found"));
    }

    #[tokio::test]
    async fn forget_missing_key() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryForgetTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool.execute(json!({"entity_id": "default"}), &ctx).await;
        assert!(result.is_err());
    }
}
