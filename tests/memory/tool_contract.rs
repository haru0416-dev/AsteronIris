use asteroniris::core::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
};
use asteroniris::core::tools::{
    ExecutionContext, MemoryForgetTool, MemoryRecallTool, MemoryStoreTool, Tool,
};
use asteroniris::security::SecurityPolicy;
use asteroniris::security::policy::TenantPolicyContext;
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

    // entity_id now falls back to ctx.entity_id when absent
    let fallback_entity = store
        .execute(
            json!({"slot_key": "profile.locale", "value": "en-US"}),
            &ctx,
        )
        .await
        .expect("should succeed with ctx.entity_id fallback");
    assert!(fallback_entity.success);

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
    let mut ctx = ExecutionContext::from_security(Arc::new(SecurityPolicy::default()));

    let recall = MemoryRecallTool::new(memory.clone());

    // Test 1: Invalid policy_context in args is silently ignored (tool succeeds)
    let invalid_recall_context = recall
        .execute(
            json!({
                "entity_id": "tenant-alpha:user-200",
                "query": "anything",
                "policy_context": "tenant-alpha"
            }),
            &ctx,
        )
        .await
        .expect("invalid policy_context in args should be ignored");
    assert!(
        invalid_recall_context.success,
        "tool should succeed when policy_context arg is invalid"
    );

    // Test 2: Invalid policy_context.tenant_mode_enabled in args is silently ignored
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
        .await
        .expect("invalid policy_context flag in args should be ignored");
    assert!(
        invalid_recall_flag.success,
        "tool should succeed when policy_context.tenant_mode_enabled is invalid"
    );

    // Test 3: Cross-tenant blocking works through ctx.tenant_context
    ctx.tenant_context = TenantPolicyContext::enabled("tenant-alpha");
    let cross_tenant_blocked = recall
        .execute(
            json!({
                "entity_id": "tenant-beta:user-200",
                "query": "anything"
            }),
            &ctx,
        )
        .await
        .expect("cross-tenant recall should return error result");
    assert!(
        !cross_tenant_blocked.success,
        "cross-tenant recall should be blocked"
    );
    assert!(
        cross_tenant_blocked.error.is_some(),
        "error should be present"
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
    // Test 4: Invalid policy_context in forget args is silently ignored
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
        .await
        .expect("invalid policy_context in forget args should be ignored");
    assert!(
        invalid_forget_context.success,
        "forget should succeed when policy_context arg is invalid"
    );
}
