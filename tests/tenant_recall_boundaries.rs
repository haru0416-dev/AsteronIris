use asteroniris::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
    SqliteMemory,
};
use asteroniris::security::policy::{
    TenantPolicyContext, TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR,
    TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR,
};
use tempfile::TempDir;

async fn seed_fact(memory: &SqliteMemory, entity_id: &str, slot_key: &str, value: &str) {
    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                slot_key,
                MemoryEventType::FactAdded,
                value,
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.75),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn tenant_recall_blocks_cross_scope() {
    let tmp = TempDir::new().unwrap();
    let memory = SqliteMemory::new(tmp.path()).unwrap();

    seed_fact(
        &memory,
        "tenant-alpha:user-001",
        "profile.language",
        "Primary language is Rust",
    )
    .await;
    seed_fact(
        &memory,
        "tenant-beta:user-001",
        "profile.language",
        "Primary language is Go",
    )
    .await;

    let allowed = memory
        .recall_scoped(
            RecallQuery::new("tenant-alpha:user-001", "language", 5)
                .with_policy_context(TenantPolicyContext::enabled("tenant-alpha")),
        )
        .await
        .unwrap();
    assert_eq!(allowed.len(), 1, "same-tenant recall should succeed");
    assert_eq!(allowed[0].entity_id, "tenant-alpha:user-001");

    let err = memory
        .recall_scoped(
            RecallQuery::new("tenant-beta:user-001", "language", 5)
                .with_policy_context(TenantPolicyContext::enabled("tenant-alpha")),
        )
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR,
        "cross-tenant recall must be denied deterministically"
    );
}

#[tokio::test]
async fn tenant_mode_disables_default_fallback() {
    let tmp = TempDir::new().unwrap();
    let memory = SqliteMemory::new(tmp.path()).unwrap();

    seed_fact(
        &memory,
        "tenant-alpha:user-002",
        "profile.timezone",
        "Timezone is UTC",
    )
    .await;

    let err = memory
        .recall_scoped(
            RecallQuery::new("default", "timezone", 5)
                .with_policy_context(TenantPolicyContext::enabled("tenant-alpha")),
        )
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR,
        "tenant mode must reject default-scope fallback"
    );
}
