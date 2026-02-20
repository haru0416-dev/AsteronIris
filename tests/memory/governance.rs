use asteroniris::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
};
use asteroniris::security::{AutonomyLevel, SecurityPolicy};
use asteroniris::tools::MemoryGovernanceTool;
use asteroniris::tools::traits::Tool;
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
async fn memory_governance_inspect_export_delete() {
    let (_temp, memory, security) = fixture();
    seed_slot(
        memory.as_ref(),
        "tenant-alpha:user-1",
        "profile.name",
        "Ada",
        PrivacyLevel::Public,
    )
    .await;
    seed_slot(
        memory.as_ref(),
        "tenant-alpha:user-1",
        "profile.email",
        "ada@example.test",
        PrivacyLevel::Private,
    )
    .await;

    let tool = MemoryGovernanceTool::new(memory.clone(), security);

    let inspect = tool
        .execute(json!({
            "action": "inspect",
            "actor": "compliance-bot",
            "entity_id": "tenant-alpha:user-1",
            "slot_keys": ["profile.name", "profile.email"]
        }))
        .await
        .expect("inspect should execute");
    assert!(inspect.success);
    let inspect_payload: serde_json::Value =
        serde_json::from_str(&inspect.output).expect("inspect output should be json");
    assert_eq!(inspect_payload["action"], "inspect");
    assert_eq!(
        inspect_payload["result"]["entity_id"],
        "tenant-alpha:user-1"
    );

    let export = tool
        .execute(json!({
            "action": "export",
            "actor": "compliance-bot",
            "entity_id": "tenant-alpha:user-1",
            "slot_keys": ["profile.name", "profile.email"]
        }))
        .await
        .expect("export should execute");
    assert!(export.success);
    let export_payload: serde_json::Value =
        serde_json::from_str(&export.output).expect("export output should be json");
    assert_eq!(export_payload["action"], "export");
    assert_eq!(export_payload["result"]["entry_count"], 2);

    let delete = tool
        .execute(json!({
            "action": "delete",
            "actor": "compliance-bot",
            "entity_id": "tenant-alpha:user-1",
            "slot_key": "profile.email",
            "mode": "hard",
            "reason": "dsar"
        }))
        .await
        .expect("delete should execute");
    assert!(delete.success);
    let delete_payload: serde_json::Value =
        serde_json::from_str(&delete.output).expect("delete output should be json");
    assert_eq!(delete_payload["action"], "delete");
    assert_eq!(delete_payload["result"]["status"], "complete");

    let deleted = memory
        .resolve_slot("tenant-alpha:user-1", "profile.email")
        .await
        .expect("resolve slot should run");
    assert!(deleted.is_none());

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
    assert_eq!(latest_record["scope"]["entity_id"], "tenant-alpha:user-1");
    assert!(latest_record["timestamp"].is_string());
}

#[tokio::test]
async fn memory_governance_export_returns_scoped_bundle() {
    let (_temp, memory, security) = fixture();
    seed_slot(
        memory.as_ref(),
        "tenant-alpha:user-2",
        "profile.nickname",
        "moon",
        PrivacyLevel::Public,
    )
    .await;
    seed_slot(
        memory.as_ref(),
        "tenant-alpha:user-2",
        "profile.ssn",
        "111-22-3333",
        PrivacyLevel::Secret,
    )
    .await;
    seed_slot(
        memory.as_ref(),
        "tenant-alpha:user-2",
        "profile.email",
        "moon@example.test",
        PrivacyLevel::Private,
    )
    .await;

    let tool = MemoryGovernanceTool::new(memory, security);
    let export = tool
        .execute(json!({
            "action": "export",
            "actor": "exporter",
            "entity_id": "tenant-alpha:user-2",
            "slot_keys": ["profile.nickname", "profile.ssn", "profile.email", "missing.key"]
        }))
        .await
        .expect("export should execute");

    assert!(export.success);
    let payload: serde_json::Value =
        serde_json::from_str(&export.output).expect("export output should be json");
    assert_eq!(
        payload["result"]["scope"]["slot_keys"]
            .as_array()
            .map(Vec::len),
        Some(4)
    );
    assert_eq!(payload["result"]["entry_count"], 3);
    assert_eq!(
        payload["result"]["missing_slot_keys"],
        json!(["missing.key"])
    );
    assert_eq!(payload["result"]["sensitive_fields_included"], false);

    let entries = payload["result"]["entries"]
        .as_array()
        .expect("entries should be array");

    let public_entry = entries
        .iter()
        .find(|entry| entry["slot_key"] == "profile.nickname")
        .expect("public entry should exist");
    assert_eq!(public_entry["value"], "moon");

    let private_entry = entries
        .iter()
        .find(|entry| entry["slot_key"] == "profile.email")
        .expect("private entry should exist");
    assert!(private_entry.get("value").is_none());
    assert_eq!(private_entry["value_redacted"], true);

    let secret_entry = entries
        .iter()
        .find(|entry| entry["slot_key"] == "profile.ssn")
        .expect("secret entry should exist");
    assert!(secret_entry.get("value").is_none());
    assert_eq!(secret_entry["value_redacted"], true);
}

#[tokio::test]
async fn memory_governance_delete_denied_is_audited() {
    let (_temp, memory, security) = fixture();
    seed_slot(
        memory.as_ref(),
        "tenant-alpha:user-3",
        "profile.region",
        "eu-west",
        PrivacyLevel::Private,
    )
    .await;

    let tool = MemoryGovernanceTool::new(memory, security);
    let denied = tool
        .execute(json!({
            "action": "delete",
            "actor": "compliance-bot",
            "entity_id": "tenant-beta:user-3",
            "slot_key": "profile.region",
            "mode": "hard",
            "policy_context": {
                "tenant_mode_enabled": true,
                "tenant_id": "tenant-alpha"
            }
        }))
        .await
        .expect("governance tool should return deny result");

    assert!(!denied.success);
    assert_eq!(
        denied.error,
        Some("blocked by security policy: tenant recall scope mismatch".to_string())
    );

    let payload: serde_json::Value =
        serde_json::from_str(&denied.output).expect("deny output should be json");
    let audit_path = payload["audit_record_path"]
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
    assert_eq!(latest_record["outcome"], "denied");
    assert_eq!(
        latest_record["message"],
        "blocked by security policy: tenant recall scope mismatch"
    );
}
