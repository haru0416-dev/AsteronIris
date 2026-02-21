use asteroniris::intelligence::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
    SqliteMemory,
};
use asteroniris::intelligence::tools::{ExecutionContext, MemoryGovernanceTool, Tool};
use asteroniris::security::{AutonomyLevel, SecurityPolicy};

use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn fixture() -> (TempDir, Arc<dyn Memory>, Arc<SecurityPolicy>) {
    let temp = TempDir::new().expect("temp dir should be created");
    let memory = SqliteMemory::new(temp.path()).expect("sqlite memory should initialize");
    let security = SecurityPolicy {
        autonomy: AutonomyLevel::Full,
        workspace_dir: temp.path().to_path_buf(),
        ..SecurityPolicy::default()
    };
    (temp, Arc::new(memory), Arc::new(security))
}

async fn seed_slot(
    memory: &dyn Memory,
    entity_id: &str,
    slot_key: &str,
    value: &str,
    privacy: PrivacyLevel,
) {
    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                slot_key,
                MemoryEventType::FactAdded,
                value,
                MemorySource::ExplicitUser,
                privacy,
            )
            .with_confidence(0.95)
            .with_importance(0.7),
        )
        .await
        .expect("seed slot should be inserted");
}

#[tokio::test]
async fn dsar_export_then_delete_flow() {
    let (_temp, memory, security) = fixture();
    let entity_id = "tenant-alpha:user-dsar";
    seed_slot(
        memory.as_ref(),
        entity_id,
        "profile.name",
        "Ada",
        PrivacyLevel::Public,
    )
    .await;
    seed_slot(
        memory.as_ref(),
        entity_id,
        "profile.email",
        "ada@example.test",
        PrivacyLevel::Private,
    )
    .await;

    let tool = MemoryGovernanceTool::new(memory.clone());
    let ctx = ExecutionContext::from_security(security);

    let inspect = tool
        .execute(
            json!({
                "action": "inspect",
                "actor": "compliance-bot",
                "entity_id": entity_id,
                "slot_keys": ["profile.name", "profile.email"]
            }),
            &ctx,
        )
        .await
        .expect("inspect should execute");
    assert!(inspect.success);
    let inspect_payload: serde_json::Value =
        serde_json::from_str(&inspect.output).expect("inspect output should be json");
    assert_eq!(inspect_payload["action"], "inspect");
    assert_eq!(inspect_payload["result"]["event_count"], 2);
    assert_eq!(
        inspect_payload["scope"]["slot_keys"],
        json!(["profile.email", "profile.name"])
    );

    let export = tool
        .execute(
            json!({
                "action": "export",
                "actor": "compliance-bot",
                "entity_id": entity_id,
                "slot_keys": ["profile.name", "profile.email"]
            }),
            &ctx,
        )
        .await
        .expect("export should execute");
    assert!(export.success);
    let export_payload: serde_json::Value =
        serde_json::from_str(&export.output).expect("export output should be json");
    assert_eq!(export_payload["action"], "export");
    assert_eq!(export_payload["result"]["entry_count"], 2);
    assert_eq!(export_payload["result"]["sensitive_fields_included"], false);

    let export_entries = export_payload["result"]["entries"]
        .as_array()
        .expect("entries should be array");
    let public_entry = export_entries
        .iter()
        .find(|entry| entry["slot_key"] == "profile.name")
        .expect("public entry should be present");
    assert_eq!(public_entry["value"], "Ada");

    let private_entry = export_entries
        .iter()
        .find(|entry| entry["slot_key"] == "profile.email")
        .expect("private entry should be present");
    assert_eq!(private_entry["value_redacted"], true);
    assert!(private_entry.get("value").is_none());

    let delete = tool
        .execute(
            json!({
                "action": "delete",
                "actor": "compliance-bot",
                "entity_id": entity_id,
                "slot_key": "profile.email",
                "mode": "hard",
                "reason": "dsar-export-delete"
            }),
            &ctx,
        )
        .await
        .expect("delete should execute");
    assert!(delete.success);

    let delete_payload: serde_json::Value =
        serde_json::from_str(&delete.output).expect("delete output should be json");
    assert_eq!(delete_payload["action"], "delete");
    assert_eq!(delete_payload["result"]["status"], "complete");
    assert_eq!(delete_payload["result"]["complete"], true);

    assert!(
        memory
            .resolve_slot(entity_id, "profile.email")
            .await
            .expect("resolve should succeed")
            .is_none(),
        "deleted slot should not resolve"
    );
    let recall = memory
        .recall_scoped(RecallQuery::new(entity_id, "example.test", 5))
        .await
        .expect("recall should run");
    assert!(
        recall.is_empty(),
        "deleted slot should not be returned in recall"
    );

    let audit_path = delete_payload["audit_record_path"]
        .as_str()
        .expect("audit path should be present");
    let audit_lines = tokio::fs::read_to_string(audit_path)
        .await
        .expect("audit file should exist");
    let latest_record: serde_json::Value = serde_json::from_str(
        audit_lines
            .lines()
            .last()
            .expect("audit should contain at least one record"),
    )
    .expect("latest audit line should be valid json");
    assert_eq!(latest_record["actor"], "compliance-bot");
    assert_eq!(latest_record["action"], "delete");
    assert_eq!(latest_record["outcome"], "allowed");
    assert_eq!(
        latest_record["scope"]["slot_keys"],
        json!(["profile.email"])
    );
}
