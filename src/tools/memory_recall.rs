use super::traits::{Tool, ToolResult};
use crate::memory::{Memory, RecallQuery};
use crate::security::policy::TenantPolicyContext;
use crate::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use serde_json::json;
use std::fmt::Write;
use std::sync::Arc;

/// Let the agent search its own memory
pub struct MemoryRecallTool {
    memory: Arc<dyn Memory>,
}

impl MemoryRecallTool {
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

    fn build_recall_request(args: &serde_json::Value) -> anyhow::Result<RecallQuery> {
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
            .ok_or_else(|| anyhow::anyhow!("Missing 'entity_id' parameter"))?;

        let request = RecallQuery::new(entity_id, query, limit)
            .with_policy_context(Self::parse_policy_context(args)?);
        request.enforce_policy()?;
        Ok(request)
    }
}

#[async_trait]
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
                    "description": "Entity id to scope recall"
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
            "required": ["entity_id", "query"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let request = match Self::build_recall_request(&args) {
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

    fn seeded_mem() -> (TempDir, Arc<dyn Memory>) {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();
        (tmp, Arc::new(mem))
    }

    #[tokio::test]
    async fn recall_empty() {
        let (_tmp, mem) = seeded_mem();
        let tool = MemoryRecallTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(json!({"entity_id": "default", "query": "anything"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("No memories found"));
    }

    #[tokio::test]
    async fn recall_finds_match() {
        let (_tmp, mem) = seeded_mem();
        mem.append_event(
            MemoryEventInput::new(
                "default",
                "lang",
                MemoryEventType::FactAdded,
                "User prefers Rust",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.6),
        )
        .await
        .unwrap();
        mem.append_event(
            MemoryEventInput::new(
                "default",
                "tz",
                MemoryEventType::FactAdded,
                "Timezone is EST",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.6),
        )
        .await
        .unwrap();

        let tool = MemoryRecallTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(json!({"entity_id": "default", "query": "Rust"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("Rust"));
        assert!(result.output.contains("Found 1"));
    }

    #[tokio::test]
    async fn recall_respects_limit() {
        let (_tmp, mem) = seeded_mem();
        for i in 0..10 {
            mem.append_event(
                MemoryEventInput::new(
                    "default",
                    format!("k{i}"),
                    MemoryEventType::FactAdded,
                    format!("Rust fact {i}"),
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_confidence(0.95)
                .with_importance(0.6),
            )
            .await
            .unwrap();
        }

        let tool = MemoryRecallTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({"entity_id": "default", "query": "Rust", "limit": 3}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        let item_lines = result
            .output
            .lines()
            .filter(|line| line.starts_with("- "))
            .count();
        assert_eq!(item_lines, 3);
    }

    #[tokio::test]
    async fn recall_missing_query() {
        let (_tmp, mem) = seeded_mem();
        let tool = MemoryRecallTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn name_and_schema() {
        let (_tmp, mem) = seeded_mem();
        let tool = MemoryRecallTool::new(mem);
        assert_eq!(tool.name(), "memory_recall");
        assert!(tool.parameters_schema()["properties"]["query"].is_object());
    }

    #[tokio::test]
    async fn recall_rejects_default_scope_when_tenant_mode_enabled() {
        let (_tmp, mem) = seeded_mem();
        let tool = MemoryRecallTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));

        let result = tool
            .execute(
                json!({
                    "entity_id": "default",
                    "query": "anything",
                    "policy_context": {
                        "tenant_mode_enabled": true,
                        "tenant_id": "tenant-alpha"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(!result.success);
        assert_eq!(
            result.error,
            Some(
                "Memory recall failed: blocked by security policy: tenant mode forbids default recall scope"
                    .to_string()
            )
        );
    }
}
