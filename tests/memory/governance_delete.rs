use asteroniris::intelligence::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
    SqliteMemory,
};
use asteroniris::intelligence::tools::{ExecutionContext, MemoryGovernanceTool, Tool};
use asteroniris::security::{AutonomyLevel, SecurityPolicy};

use rusqlite::Connection;
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
async fn dsar_delete_completeness_detects_residue() {
    let (temp, memory, security) = fixture();
    let entity_id = "tenant-alpha:user-residue";
    let slot_key = "profile.email";
    seed_slot(
        memory.as_ref(),
        entity_id,
        slot_key,
        "residue@example.test",
        PrivacyLevel::Private,
    )
    .await;

    let db_path = temp.path().join("memory").join("brain.db");
    let conn = Connection::open(db_path).expect("sqlite db should be accessible");
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS test_reinsert_retrieval_residue
         AFTER DELETE ON retrieval_docs
         WHEN old.doc_id = 'tenant-alpha:user-residue:profile.email'
         BEGIN
             INSERT INTO retrieval_docs (
                 doc_id,
                 entity_id,
                 slot_key,
                 text_body,
                 layer,
                 provenance_source_class,
                 provenance_reference,
                 provenance_evidence_uri,
                 retention_tier,
                 retention_expires_at,
                 recency_score,
                 importance,
                 reliability,
                 contradiction_penalty,
                 visibility,
                 updated_at
             ) VALUES (
                 old.doc_id,
                 old.entity_id,
                 old.slot_key,
                 old.text_body,
                 old.layer,
                 old.provenance_source_class,
                 old.provenance_reference,
                 old.provenance_evidence_uri,
                 old.retention_tier,
                 old.retention_expires_at,
                 old.recency_score,
                 old.importance,
                 old.reliability,
                 old.contradiction_penalty,
                 old.visibility,
                 old.updated_at
             );
         END;",
    )
    .expect("residue trigger should be created");

    let tool = MemoryGovernanceTool::new(memory.clone());
    let ctx = ExecutionContext::from_security(security);
    let delete = tool
        .execute(
            json!({
                "action": "delete",
                "actor": "compliance-bot",
                "entity_id": entity_id,
                "slot_key": slot_key,
                "mode": "hard",
                "reason": "dsar-residue-fixture"
            }),
            &ctx,
        )
        .await
        .expect("delete should execute");
    assert!(delete.success, "tool call should complete with payload");

    let payload: serde_json::Value =
        serde_json::from_str(&delete.output).expect("delete output should be json");
    assert_eq!(payload["action"], "delete");
    assert_eq!(payload["result"]["applied"], true);
    assert_eq!(payload["result"]["complete"], false);
    assert_eq!(payload["result"]["status"], "incomplete");

    let residue_check = payload["result"]["artifact_checks"]
        .as_array()
        .expect("artifact checks should be array")
        .iter()
        .find(|check| check["artifact"] == "retrieval_docs")
        .expect("retrieval docs artifact check should exist");
    assert_eq!(residue_check["satisfied"], false);
    assert_eq!(residue_check["observed"], "present_retrievable");

    assert!(
        memory
            .resolve_slot(entity_id, slot_key)
            .await
            .expect("resolve should succeed")
            .is_none(),
        "hard delete still removes belief slot"
    );
    let recall = memory
        .recall_scoped(RecallQuery::new(entity_id, "example.test", 5))
        .await
        .expect("recall should run");
    assert!(
        recall.is_empty(),
        "ledger denylist should prevent replay even when residue exists"
    );

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
    assert_eq!(latest_record["action"], "delete");
    assert_eq!(latest_record["outcome"], "allowed");
}
