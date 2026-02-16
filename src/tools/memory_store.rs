use super::traits::{Tool, ToolResult};
use crate::memory::{Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

/// Let the agent store memories — its own brain writes
pub struct MemoryStoreTool {
    memory: Arc<dyn Memory>,
}

impl MemoryStoreTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str {
        "memory_store"
    }

    fn description(&self) -> &str {
        "Append one immutable memory event for an entity slot."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "entity_id": {
                    "type": "string",
                    "description": "Entity identifier"
                },
                "slot_key": {
                    "type": "string",
                    "description": "Slot key"
                },
                "value": {
                    "type": "string",
                    "description": "Slot value to persist"
                },
                "event_type": {
                    "type": "string",
                    "description": "Event type (e.g. preference_set, fact_updated)"
                },
                "source": {
                    "type": "string",
                    "enum": ["explicit_user", "tool_verified", "system", "inferred"],
                    "description": "Event source"
                },
                "confidence": {
                    "type": "number",
                    "description": "Confidence score 0..1"
                },
                "importance": {
                    "type": "number",
                    "description": "Importance score 0..1"
                },
                "privacy_level": {
                    "type": "string",
                    "enum": ["public", "private", "secret"],
                    "description": "Privacy label"
                }
            },
            "required": ["entity_id", "slot_key", "value"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let entity_id = args
            .get("entity_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'entity_id' parameter"))?;

        let value = args
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'value' parameter"))?;

        let slot_key = args
            .get("slot_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'slot_key' parameter"))?
            .to_string();

        let event_type = args
            .get("event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("fact_added")
            .parse::<MemoryEventType>()?;

        let source = match args.get("source").and_then(|v| v.as_str()) {
            Some("explicit_user") => MemorySource::ExplicitUser,
            Some("tool_verified") => MemorySource::ToolVerified,
            Some("inferred") => MemorySource::Inferred,
            _ => MemorySource::System,
        };

        let privacy_level = match args.get("privacy_level").and_then(|v| v.as_str()) {
            Some("public") => PrivacyLevel::Public,
            Some("secret") => PrivacyLevel::Secret,
            _ => PrivacyLevel::Private,
        };

        let confidence = args
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.8)
            .clamp(0.0, 1.0);

        let importance = args
            .get("importance")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);

        let input = MemoryEventInput::new(
            entity_id,
            slot_key,
            event_type,
            value,
            source,
            privacy_level,
        )
        .with_confidence(confidence)
        .with_importance(importance);

        match self.memory.append_event(input).await {
            Ok(event) => Ok(ToolResult {
                success: true,
                output: format!("Stored memory event: {}", event.event_id),
                error: None,
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to store memory: {e}")),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::SqliteMemory;
    use tempfile::TempDir;

    fn test_mem() -> (TempDir, Arc<dyn Memory>) {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();
        (tmp, Arc::new(mem))
    }

    #[test]
    fn name_and_schema() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem);
        assert_eq!(tool.name(), "memory_store");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["entity_id"].is_object());
        assert!(schema["properties"]["slot_key"].is_object());
        assert!(schema["properties"]["value"].is_object());
    }

    #[tokio::test]
    async fn store_core() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem.clone());
        let result = tool
            .execute(json!({"entity_id": "lang", "slot_key": "note", "value": "Prefers Rust"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("Stored memory event"));

        let slot = mem.resolve_slot("lang", "note").await.unwrap();
        assert!(slot.is_some());
        assert_eq!(slot.unwrap().value, "Prefers Rust");
    }

    #[tokio::test]
    async fn store_with_category() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem.clone());
        let result = tool
            .execute(json!({"entity_id": "note", "slot_key": "daily", "value": "Fixed bug"}))
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn store_missing_key() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem);
        let result = tool
            .execute(json!({"slot_key": "x", "value": "no key"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn store_missing_content() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem);
        let result = tool
            .execute(json!({"entity_id": "no_content", "slot_key": "x"}))
            .await;
        assert!(result.is_err());
    }
}
