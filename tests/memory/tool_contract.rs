use asteroniris::intelligence::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
};
use asteroniris::intelligence::tools::{
    ExecutionContext, MemoryForgetTool, MemoryRecallTool, MemoryStoreTool, Tool,
};
use asteroniris::security::SecurityPolicy;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn sqlite_memory() -> (TempDir, Arc<dyn Memory>) {
    let temp = TempDir::new().expect("temp dir should be created");
    let memory = SqliteMemory::new(temp.path()).expect("sqlite memory should initialize");
    (temp, Arc::new(memory))
}

#[tokio::test]
async fn memory_tool_schema_backward_compat() {
    let (_temp, memory) = sqlite_memory();
    let ctx = ExecutionContext::from_security(Arc::new(SecurityPolicy::default()));

    let store = MemoryStoreTool::new(memory.clone());
    let store_result = store
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-100",
                "slot_key": "profile.language",
                "value": "Rust"
            }),
            &ctx,
        )
        .await
        .expect("legacy store payload should execute");
    assert!(store_result.success);

    let recall = MemoryRecallTool::new(memory.clone());
    let recall_result = recall
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-100",
                "query": "Rust"
            }),
            &ctx,
        )
        .await
        .expect("legacy recall payload should execute");
    assert!(recall_result.success);
    assert!(recall_result.output.contains("Rust"));

    memory
        .append_event(
            MemoryEventInput::new(
                "tenant-alpha:user-100",
                "legacy.slot",
                MemoryEventType::FactAdded,
                "legacy value",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_importance(0.5),
        )
        .await
        .expect("seed event should be inserted");

    let forget = MemoryForgetTool::new(memory);
    let forget_result = forget
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-100",
                "key": "legacy.slot"
            }),
            &ctx,
        )
        .await
        .expect("legacy forget payload shape should execute");
    assert!(forget_result.success);

    let missing_entity = store
        .execute(
            json!({"slot_key": "profile.locale", "value": "en-US"}),
            &ctx,
        )
        .await;
    assert!(missing_entity.is_err());
    assert_eq!(
        missing_entity
            .expect_err("missing entity should fail")
            .to_string(),
        "Missing 'entity_id' parameter"
    );

    let missing_slot_key = forget
        .execute(json!({"entity_id": "tenant-alpha:user-100"}), &ctx)
        .await;
    assert!(missing_slot_key.is_err());
    assert_eq!(
        missing_slot_key
            .expect_err("missing slot key should fail")
            .to_string(),
        "Missing 'slot_key' parameter"
    );

    let missing_query = recall
        .execute(json!({"entity_id": "tenant-alpha:user-100"}), &ctx)
        .await;
    assert!(missing_query.is_err());
    assert_eq!(
        missing_query
            .expect_err("missing query should fail")
            .to_string(),
        "Missing 'query' parameter"
    );
}

#[tokio::test]
async fn memory_tool_policy_context_validation() {
    let (_temp, memory) = sqlite_memory();
    let ctx = ExecutionContext::from_security(Arc::new(SecurityPolicy::default()));

    let recall = MemoryRecallTool::new(memory.clone());
    let invalid_recall_context = recall
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-200",
                "query": "anything",
                "policy_context": "tenant-alpha"
            }),
            &ctx,
        )
        .await;
    assert!(invalid_recall_context.is_err());
    assert_eq!(
        invalid_recall_context
            .expect_err("invalid policy context should fail")
            .to_string(),
        "Invalid 'policy_context' parameter: expected object"
    );

    let invalid_recall_flag = recall
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-200",
                "query": "anything",
                "policy_context": {
                    "tenant_mode_enabled": "yes",
                    "tenant_id": "tenant-alpha"
                }
            }),
            &ctx,
        )
        .await;
    assert!(invalid_recall_flag.is_err());
    assert_eq!(
        invalid_recall_flag
            .expect_err("invalid policy context flag should fail")
            .to_string(),
        "Invalid 'policy_context.tenant_mode_enabled' parameter: expected boolean"
    );

    let store = MemoryStoreTool::new(memory.clone());
    let invalid_provenance = store
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-200",
                "slot_key": "profile.timezone",
                "value": "UTC",
                "provenance": {
                    "source_class": "invalid",
                    "reference": "ticket:11"
                }
            }),
            &ctx,
        )
        .await;
    assert!(invalid_provenance.is_err());
    assert_eq!(
        invalid_provenance
            .expect_err("invalid provenance source should fail")
            .to_string(),
        "Invalid 'provenance.source_class' parameter: must be one of explicit_user, tool_verified, system, inferred"
    );

    let empty_provenance_reference = store
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-200",
                "slot_key": "profile.timezone",
                "value": "UTC",
                "provenance": {
                    "source_class": "system",
                    "reference": "   "
                }
            }),
            &ctx,
        )
        .await;
    assert!(empty_provenance_reference.is_err());
    assert_eq!(
        empty_provenance_reference
            .expect_err("empty provenance reference should fail")
            .to_string(),
        "Invalid 'provenance.reference' parameter: must not be empty"
    );

    let forget = MemoryForgetTool::new(memory);
    let invalid_forget_context = forget
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-200",
                "slot_key": "profile.timezone",
                "policy_context": {
                    "tenant_mode_enabled": true,
                    "tenant_id": 123
                }
            }),
            &ctx,
        )
        .await;
    assert!(invalid_forget_context.is_err());
    assert_eq!(
        invalid_forget_context
            .expect_err("invalid forget context should fail")
            .to_string(),
        "Invalid 'policy_context.tenant_id' parameter: expected string or null"
    );
}
