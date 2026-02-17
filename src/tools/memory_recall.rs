use super::traits::{Tool, ToolResult};
use crate::memory::{Memory, RecallQuery};
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
                }
            },
            "required": ["entity_id", "query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
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

        let request = RecallQuery::new(entity_id, query, limit);

        match self.memory.recall_scoped(request).await {
            Ok(entries) if entries.is_empty() => Ok(ToolResult {
                success: true,
                output: "No memories found matching that query.".into(),
                error: None,
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
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Memory recall failed: {e}")),
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
        let result = tool
            .execute(json!({"entity_id": "default", "query": "anything"}))
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
        let result = tool
            .execute(json!({"entity_id": "default", "query": "Rust"}))
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
        let result = tool
            .execute(json!({"entity_id": "default", "query": "Rust", "limit": 3}))
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
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn name_and_schema() {
        let (_tmp, mem) = seeded_mem();
        let tool = MemoryRecallTool::new(mem);
        assert_eq!(tool.name(), "memory_recall");
        assert!(tool.parameters_schema()["properties"]["query"].is_object());
    }
}
