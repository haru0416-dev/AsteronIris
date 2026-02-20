use super::traits::{Tool, ToolResult};
use crate::memory::traits::MemoryLayer;
use crate::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
};
use crate::tools::middleware::ExecutionContext;
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

    fn parse_layer(args: &serde_json::Value) -> anyhow::Result<MemoryLayer> {
        let Some(layer) = args.get("layer") else {
            return Ok(MemoryLayer::Working);
        };

        let Some(layer) = layer.as_str() else {
            anyhow::bail!("Invalid 'layer' parameter: expected string");
        };

        match layer {
            "working" => Ok(MemoryLayer::Working),
            "episodic" => Ok(MemoryLayer::Episodic),
            "semantic" => Ok(MemoryLayer::Semantic),
            "procedural" => Ok(MemoryLayer::Procedural),
            "identity" => Ok(MemoryLayer::Identity),
            _ => anyhow::bail!(
                "Invalid 'layer' parameter: must be one of working, episodic, semantic, procedural, identity"
            ),
        }
    }

    fn parse_provenance(args: &serde_json::Value) -> anyhow::Result<Option<MemoryProvenance>> {
        let Some(raw_provenance) = args.get("provenance") else {
            return Ok(None);
        };

        let Some(provenance) = raw_provenance.as_object() else {
            anyhow::bail!("Invalid 'provenance' parameter: expected object");
        };

        let source_class = provenance.get("source_class").ok_or_else(|| {
            anyhow::anyhow!("Invalid 'provenance.source_class' parameter: missing required field")
        })?;

        let Some(source_class) = source_class.as_str() else {
            anyhow::bail!("Invalid 'provenance.source_class' parameter: expected string");
        };

        let source_class = match source_class {
            "explicit_user" => MemorySource::ExplicitUser,
            "tool_verified" => MemorySource::ToolVerified,
            "system" => MemorySource::System,
            "inferred" => MemorySource::Inferred,
            _ => anyhow::bail!(
                "Invalid 'provenance.source_class' parameter: must be one of explicit_user, tool_verified, system, inferred"
            ),
        };

        let reference = provenance.get("reference").ok_or_else(|| {
            anyhow::anyhow!("Invalid 'provenance.reference' parameter: missing required field")
        })?;

        let Some(reference) = reference.as_str() else {
            anyhow::bail!("Invalid 'provenance.reference' parameter: expected string");
        };

        if reference.trim().is_empty() {
            anyhow::bail!("Invalid 'provenance.reference' parameter: must not be empty");
        }

        let evidence_uri = match provenance.get("evidence_uri") {
            Some(serde_json::Value::Null) | None => None,
            Some(value) => {
                let Some(uri) = value.as_str() else {
                    anyhow::bail!("Invalid 'provenance.evidence_uri' parameter: expected string");
                };
                if uri.trim().is_empty() {
                    anyhow::bail!("Invalid 'provenance.evidence_uri' parameter: must not be empty");
                }
                Some(uri.to_string())
            }
        };

        Ok(Some(MemoryProvenance {
            source_class,
            reference: reference.to_string(),
            evidence_uri,
        }))
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
                "layer": {
                    "type": "string",
                    "enum": ["working", "episodic", "semantic", "procedural", "identity"],
                    "description": "Memory layer (defaults to working)"
                },
                "source": {
                    "type": "string",
                    "enum": ["explicit_user", "tool_verified", "system", "inferred"],
                    "description": "Event source"
                },
                "confidence": {
                    "type": "number",
                    "description": "Confidence score 0..1 (defaults by source class when omitted)"
                },
                "importance": {
                    "type": "number",
                    "description": "Importance score 0..1"
                },
                "provenance": {
                    "type": "object",
                    "description": "Optional provenance source reference envelope",
                    "properties": {
                        "source_class": {
                            "type": "string",
                            "enum": ["explicit_user", "tool_verified", "system", "inferred"]
                        },
                        "reference": {
                            "type": "string",
                            "description": "Stable source reference (ticket, event id, trace id, etc.)"
                        },
                        "evidence_uri": {
                            "type": "string",
                            "description": "Optional supporting URI"
                        }
                    },
                    "required": ["source_class", "reference"]
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

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
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

        let layer = Self::parse_layer(&args)?;

        let privacy_level = match args.get("privacy_level").and_then(|v| v.as_str()) {
            Some("public") => PrivacyLevel::Public,
            Some("secret") => PrivacyLevel::Secret,
            _ => PrivacyLevel::Private,
        };

        let confidence = args
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .map(|value| value.clamp(0.0, 1.0));

        let importance = args
            .get("importance")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);

        let provenance = Self::parse_provenance(&args)?;

        let mut input = MemoryEventInput::new(
            entity_id,
            slot_key,
            event_type,
            value,
            source,
            privacy_level,
        )
        .with_layer(layer)
        .with_importance(importance);

        if let Some(confidence) = confidence {
            input = input.with_confidence(confidence);
        }

        if let Some(provenance) = provenance {
            input = input.with_provenance(provenance);
        }

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
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({"entity_id": "lang", "slot_key": "note", "value": "Prefers Rust"}),
                &ctx,
            )
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
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({"entity_id": "note", "slot_key": "daily", "value": "Fixed bug"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn store_missing_key() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(json!({"slot_key": "x", "value": "no key"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn store_missing_content() {
        let (_tmp, mem) = test_mem();
        let tool = MemoryStoreTool::new(mem);
        let ctx =
            ExecutionContext::test_default(Arc::new(crate::security::SecurityPolicy::default()));
        let result = tool
            .execute(json!({"entity_id": "no_content", "slot_key": "x"}), &ctx)
            .await;
        assert!(result.is_err());
    }
}
