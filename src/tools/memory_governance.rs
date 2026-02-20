use super::traits::{Tool, ToolResult};
use crate::memory::{BeliefSlot, ForgetMode, Memory, PrivacyLevel};
use crate::security::policy::TenantPolicyContext;
use crate::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

enum GovernanceAction {
    Inspect,
    Export,
    Delete,
}

impl GovernanceAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Export => "export",
            Self::Delete => "delete",
        }
    }
}

pub struct MemoryGovernanceTool {
    memory: Arc<dyn Memory>,
}

impl MemoryGovernanceTool {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self { memory }
    }

    fn parse_action(args: &serde_json::Value) -> anyhow::Result<GovernanceAction> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;
        match action {
            "inspect" => Ok(GovernanceAction::Inspect),
            "export" => Ok(GovernanceAction::Export),
            "delete" => Ok(GovernanceAction::Delete),
            _ => {
                anyhow::bail!("Invalid 'action' parameter: must be one of inspect, export, delete")
            }
        }
    }

    fn parse_mode(args: &serde_json::Value) -> ForgetMode {
        match args.get("mode").and_then(|v| v.as_str()) {
            Some("hard") => ForgetMode::Hard,
            Some("tombstone") => ForgetMode::Tombstone,
            _ => ForgetMode::Soft,
        }
    }

    fn parse_actor(args: &serde_json::Value) -> anyhow::Result<String> {
        let actor = args
            .get("actor")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'actor' parameter"))?
            .trim();
        if actor.is_empty() {
            anyhow::bail!("Invalid 'actor' parameter: must not be empty");
        }
        Ok(actor.to_string())
    }

    fn parse_entity_id(args: &serde_json::Value) -> anyhow::Result<String> {
        let entity_id = args
            .get("entity_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'entity_id' parameter"))?
            .trim();
        if entity_id.is_empty() {
            anyhow::bail!("Invalid 'entity_id' parameter: must not be empty");
        }
        Ok(entity_id.to_string())
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

    fn parse_scope_keys(args: &serde_json::Value) -> anyhow::Result<Vec<String>> {
        let mut keys = Vec::new();

        if let Some(slot_key) = args.get("slot_key") {
            let Some(slot_key) = slot_key.as_str() else {
                anyhow::bail!("Invalid 'slot_key' parameter: expected string");
            };
            let slot_key = slot_key.trim();
            if slot_key.is_empty() {
                anyhow::bail!("Invalid 'slot_key' parameter: must not be empty");
            }
            keys.push(slot_key.to_string());
        }

        if let Some(raw_slot_keys) = args.get("slot_keys") {
            let Some(raw_slot_keys) = raw_slot_keys.as_array() else {
                anyhow::bail!("Invalid 'slot_keys' parameter: expected array of strings");
            };
            for value in raw_slot_keys {
                let Some(slot_key) = value.as_str() else {
                    anyhow::bail!("Invalid 'slot_keys' parameter: expected array of strings");
                };
                let slot_key = slot_key.trim();
                if slot_key.is_empty() {
                    anyhow::bail!("Invalid 'slot_keys' parameter: must not contain empty values");
                }
                keys.push(slot_key.to_string());
            }
        }

        keys.sort_unstable();
        keys.dedup();
        Ok(keys)
    }

    fn parse_include_sensitive(args: &serde_json::Value) -> bool {
        args.get("include_sensitive")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    }

    fn redact_slot(slot: &BeliefSlot, include_sensitive: bool) -> serde_json::Value {
        let can_include_value =
            include_sensitive || matches!(slot.privacy_level, PrivacyLevel::Public);

        if can_include_value {
            json!({
                "slot_key": slot.slot_key,
                "privacy_level": slot.privacy_level,
                "value": slot.value,
                "confidence": slot.confidence,
                "importance": slot.importance,
                "updated_at": slot.updated_at,
            })
        } else {
            json!({
                "slot_key": slot.slot_key,
                "privacy_level": slot.privacy_level,
                "value_redacted": true,
                "confidence": slot.confidence,
                "importance": slot.importance,
                "updated_at": slot.updated_at,
            })
        }
    }

    fn audit_path(workspace_dir: &Path) -> PathBuf {
        let date = Utc::now().format("%Y-%m-%d").to_string();
        workspace_dir
            .join("memory_governance")
            .join(format!("{date}.jsonl"))
    }

    async fn append_audit_record(
        &self,
        actor: &str,
        action: &GovernanceAction,
        entity_id: &str,
        scope_keys: &[String],
        status: (&str, &str),
        workspace_dir: &Path,
    ) -> anyhow::Result<String> {
        let (outcome, message) = status;
        let path = Self::audit_path(workspace_dir);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        let record = json!({
            "timestamp": Utc::now().to_rfc3339(),
            "actor": actor,
            "action": action.as_str(),
            "scope": {
                "entity_id": entity_id,
                "slot_keys": scope_keys,
            },
            "outcome": outcome,
            "message": message,
        });

        file.write_all(record.to_string().as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(path.to_string_lossy().to_string())
    }

    async fn run_inspect(
        &self,
        entity_id: &str,
        scope_keys: &[String],
        include_sensitive: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let event_count = self.memory.count_events(Some(entity_id)).await?;
        if scope_keys.is_empty() {
            return Ok(json!({
                "entity_id": entity_id,
                "event_count": event_count,
                "inspected_slots": [],
            }));
        }

        let mut inspected_slots = Vec::new();
        for slot_key in scope_keys {
            let slot = self.memory.resolve_slot(entity_id, slot_key).await?;
            if let Some(slot) = slot {
                inspected_slots.push(Self::redact_slot(&slot, include_sensitive));
            } else {
                inspected_slots.push(json!({
                    "slot_key": slot_key,
                    "status": "not_found",
                }));
            }
        }

        Ok(json!({
            "entity_id": entity_id,
            "event_count": event_count,
            "inspected_slots": inspected_slots,
        }))
    }

    async fn run_export(
        &self,
        entity_id: &str,
        scope_keys: &[String],
        include_sensitive: bool,
    ) -> anyhow::Result<serde_json::Value> {
        if scope_keys.is_empty() {
            anyhow::bail!("Missing scope for export: provide 'slot_key' or 'slot_keys'");
        }

        let mut entries = Vec::new();
        let mut missing_slot_keys = Vec::new();
        for slot_key in scope_keys {
            match self.memory.resolve_slot(entity_id, slot_key).await? {
                Some(slot) => entries.push(Self::redact_slot(&slot, include_sensitive)),
                None => missing_slot_keys.push(slot_key.clone()),
            }
        }

        Ok(json!({
            "entity_id": entity_id,
            "scope": {
                "slot_keys": scope_keys,
            },
            "entry_count": entries.len(),
            "entries": entries,
            "missing_slot_keys": missing_slot_keys,
            "sensitive_fields_included": include_sensitive,
        }))
    }

    async fn run_delete(
        &self,
        entity_id: &str,
        scope_keys: &[String],
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let Some(slot_key) = scope_keys.first() else {
            anyhow::bail!("Missing scope for delete: provide 'slot_key'");
        };
        let outcome = self
            .memory
            .forget_slot(entity_id, slot_key, mode, reason)
            .await?;
        Ok(serde_json::to_value(outcome)?)
    }
}

#[async_trait]
impl Tool for MemoryGovernanceTool {
    fn name(&self) -> &str {
        "memory_governance"
    }

    fn description(&self) -> &str {
        "Run governance inspect/export/delete actions on memory with audit logging."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["inspect", "export", "delete"],
                    "description": "Governance action type"
                },
                "actor": {
                    "type": "string",
                    "description": "Actor identifier for audit records"
                },
                "entity_id": {
                    "type": "string",
                    "description": "Entity id scope"
                },
                "slot_key": {
                    "type": "string",
                    "description": "Single slot scope key"
                },
                "slot_keys": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Scoped slot keys for inspect/export"
                },
                "mode": {
                    "type": "string",
                    "enum": ["soft", "hard", "tombstone"],
                    "description": "Delete mode for action=delete"
                },
                "reason": {
                    "type": "string",
                    "description": "Delete reason for action=delete"
                },
                "include_sensitive": {
                    "type": "boolean",
                    "description": "Include private/secret values in inspect/export responses"
                },
                "policy_context": {
                    "type": "object",
                    "description": "Optional tenant policy context to validate governance scope",
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
            "required": ["action", "actor", "entity_id"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let action = Self::parse_action(&args)?;
        let actor = Self::parse_actor(&args)?;
        let entity_id = Self::parse_entity_id(&args)?;
        let scope_keys = Self::parse_scope_keys(&args)?;
        let include_sensitive = Self::parse_include_sensitive(&args);
        let policy_context = Self::parse_policy_context(&args)?;

        if let Err(error) = policy_context.enforce_recall_scope(&entity_id) {
            let audit_record_path = self
                .append_audit_record(
                    &actor,
                    &action,
                    &entity_id,
                    &scope_keys,
                    ("denied", error),
                    &ctx.workspace_dir,
                )
                .await?;
            return Ok(ToolResult {
                success: false,
                output: json!({
                    "audit_record_path": audit_record_path,
                    "action": action.as_str(),
                    "entity_id": entity_id,
                    "scope": { "slot_keys": scope_keys },
                })
                .to_string(),
                error: Some(error.to_string()),
            });
        }

        let payload = match action {
            GovernanceAction::Inspect => {
                self.run_inspect(&entity_id, &scope_keys, include_sensitive)
                    .await?
            }
            GovernanceAction::Export => {
                self.run_export(&entity_id, &scope_keys, include_sensitive)
                    .await?
            }
            GovernanceAction::Delete => {
                let mode = Self::parse_mode(&args);
                let reason = args
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("governance_request");
                self.run_delete(&entity_id, &scope_keys, mode, reason)
                    .await?
            }
        };

        let audit_record_path = self
            .append_audit_record(
                &actor,
                &action,
                &entity_id,
                &scope_keys,
                ("allowed", "governance action completed"),
                &ctx.workspace_dir,
            )
            .await?;

        let output = json!({
            "action": action.as_str(),
            "entity_id": entity_id,
            "scope": { "slot_keys": scope_keys },
            "result": payload,
            "audit_record_path": audit_record_path,
        });

        Ok(ToolResult {
            success: true,
            output: output.to_string(),
            error: None,
        })
    }
}
