use arrow_array::{Float64Array, StringArray};
use asteroniris::config::MemoryConfig;
use asteroniris::memory::traits::MemoryLayer;
use asteroniris::memory::{
    backend_capability_matrix, capability_matrix_for_backend, capability_matrix_for_memory,
    create_memory, ensure_forget_mode_supported, CapabilitySupport, ForgetMode, Memory,
    MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel, RecallQuery,
};
use futures_util::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use tempfile::TempDir;

use super::memory_harness;

#[test]
fn memory_capability_matrix() {
    let matrix = backend_capability_matrix();
    assert_eq!(matrix.len(), 3);

    let sqlite = capability_matrix_for_backend("sqlite").expect("sqlite capability row");
    assert_eq!(sqlite.forget_soft, CapabilitySupport::Supported);
    assert_eq!(sqlite.forget_hard, CapabilitySupport::Supported);
    assert_eq!(sqlite.forget_tombstone, CapabilitySupport::Supported);

    let lancedb = capability_matrix_for_backend("lancedb").expect("lancedb capability row");
    assert_eq!(lancedb.forget_soft, CapabilitySupport::Degraded);
    assert_eq!(lancedb.forget_hard, CapabilitySupport::Supported);
    assert_eq!(lancedb.forget_tombstone, CapabilitySupport::Degraded);

    let markdown = capability_matrix_for_backend("markdown").expect("markdown capability row");
    assert_eq!(markdown.forget_soft, CapabilitySupport::Degraded);
    assert_eq!(markdown.forget_hard, CapabilitySupport::Unsupported);
    assert_eq!(markdown.forget_tombstone, CapabilitySupport::Degraded);
}

#[test]
fn memory_capability_rejects_unsupported() {
    let markdown = capability_matrix_for_backend("markdown").expect("markdown capability row");
    let err = markdown
        .require_forget_mode(ForgetMode::Hard)
        .expect_err("hard delete must be rejected for markdown backend contract");

    assert_eq!(
        err.to_string(),
        "memory backend 'markdown' does not support forget mode 'hard'"
    );

    let lancedb = capability_matrix_for_backend("lancedb").expect("lancedb capability row");
    lancedb
        .require_forget_mode(ForgetMode::Hard)
        .expect("lancedb supports hard delete in contract");
}

#[test]
fn memory_capability_matrix_runtime_access() {
    let tmp = TempDir::new().expect("temp dir");
    let markdown_cfg = MemoryConfig {
        backend: "markdown".into(),
        ..MemoryConfig::default()
    };
    let markdown_memory = create_memory(&markdown_cfg, tmp.path(), None).expect("markdown memory");
    let markdown_caps = capability_matrix_for_memory(markdown_memory.as_ref());
    assert_eq!(markdown_caps.backend, "markdown");

    let hard_delete_err = ensure_forget_mode_supported(markdown_memory.as_ref(), ForgetMode::Hard)
        .expect_err("markdown hard forget should be rejected by capability preflight");
    assert_eq!(
        hard_delete_err.to_string(),
        "memory backend 'markdown' does not support forget mode 'hard'"
    );

    let sqlite_cfg = MemoryConfig {
        backend: "sqlite".into(),
        ..MemoryConfig::default()
    };
    let sqlite_memory = create_memory(&sqlite_cfg, tmp.path(), None).expect("sqlite memory");
    let sqlite_caps = capability_matrix_for_memory(sqlite_memory.as_ref());
    assert_eq!(sqlite_caps.backend, "sqlite");
    ensure_forget_mode_supported(sqlite_memory.as_ref(), ForgetMode::Hard)
        .expect("sqlite hard forget should remain supported");
}

#[tokio::test]
async fn lancedb_capability_contract() {
    let lancedb = capability_matrix_for_backend("lancedb").expect("lancedb capability row");
    assert_eq!(lancedb.forget_soft, CapabilitySupport::Degraded);
    assert_eq!(lancedb.forget_hard, CapabilitySupport::Supported);
    assert_eq!(lancedb.forget_tombstone, CapabilitySupport::Degraded);
    assert_eq!(
        lancedb.unsupported_contract,
        "lancedb soft/tombstone are marker rewrites; hard forget removes projection"
    );

    let (_tmp, memory) = memory_harness::lancedb_fixture();
    memory_harness::append_test_event(
        &memory,
        "contract-entity",
        "slot",
        "seed value",
        asteroniris::memory::MemoryCategory::Core,
    )
    .await;
    ensure_forget_mode_supported(&memory, ForgetMode::Soft)
        .expect("degraded support still allows contract execution");
    ensure_forget_mode_supported(&memory, ForgetMode::Tombstone)
        .expect("degraded support still allows contract execution");

    let hard = memory
        .forget_slot(
            "contract-entity",
            "slot",
            ForgetMode::Hard,
            "contract-check",
        )
        .await
        .expect("hard forget should execute");
    assert!(hard.applied);

    let removed = memory
        .resolve_slot("contract-entity", "slot")
        .await
        .expect("resolve after hard forget should succeed");
    assert!(removed.is_none());
}

#[tokio::test]
async fn lancedb_persists_supported_memory_fields() {
    let tmp = TempDir::new().expect("temp dir");
    let memory = memory_harness::lancedb_memory_from_path(tmp.path());

    let input = MemoryEventInput::new(
        "entity-lancedb",
        "profile.preference",
        MemoryEventType::FactAdded,
        "prefers deterministic markers",
        MemorySource::ToolVerified,
        PrivacyLevel::Secret,
    )
    .with_confidence(0.41)
    .with_importance(0.73)
    .with_layer(MemoryLayer::Identity)
    .with_provenance(
        MemoryProvenance::source_reference(MemorySource::ToolVerified, "audit:task-9")
            .with_evidence_uri("https://example.test/task-9"),
    )
    .with_occurred_at("2026-02-18T11:22:33Z");

    memory
        .append_event(input)
        .await
        .expect("lancedb append should persist supported fields");

    let resolved = memory
        .resolve_slot("entity-lancedb", "profile.preference")
        .await
        .expect("resolve should succeed")
        .expect("slot should exist");
    assert_eq!(resolved.source, MemorySource::ToolVerified);
    assert!((resolved.confidence - 0.41).abs() < f64::EPSILON);
    assert!((resolved.importance - 0.73).abs() < f64::EPSILON);
    assert_eq!(resolved.privacy_level, PrivacyLevel::Secret);

    let recalled = memory
        .recall_scoped(RecallQuery::new("entity-lancedb", "deterministic", 1))
        .await
        .expect("recall should succeed");
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].source, MemorySource::ToolVerified);
    assert_eq!(recalled[0].occurred_at, "2026-02-18T11:22:33Z");

    let db_path = tmp.path().join("memory").join("lancedb");
    let conn = lancedb::connect(db_path.to_string_lossy().as_ref())
        .execute()
        .await
        .expect("connect lancedb");
    let table = conn
        .open_table("memories")
        .execute()
        .await
        .expect("open memories table");
    let mut stream = table
        .query()
        .only_if("key = 'entity-lancedb:profile.preference'")
        .limit(1)
        .select(Select::columns(&[
            "layer",
            "provenance_source_class",
            "provenance_reference",
            "provenance_evidence_uri",
            "confidence",
            "importance",
            "privacy_level",
            "occurred_at",
        ]))
        .execute()
        .await
        .expect("query persisted row");

    let batch = stream
        .try_next()
        .await
        .expect("read query stream")
        .expect("expected one batch");
    let layer = batch
        .column_by_name("layer")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .expect("layer column");
    let provenance_source = batch
        .column_by_name("provenance_source_class")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .expect("provenance source column");
    let provenance_reference = batch
        .column_by_name("provenance_reference")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .expect("provenance reference column");
    let provenance_evidence = batch
        .column_by_name("provenance_evidence_uri")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .expect("provenance evidence column");
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|col| col.as_any().downcast_ref::<Float64Array>())
        .expect("confidence column");
    let importance = batch
        .column_by_name("importance")
        .and_then(|col| col.as_any().downcast_ref::<Float64Array>())
        .expect("importance column");
    let privacy = batch
        .column_by_name("privacy_level")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .expect("privacy_level column");
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .expect("occurred_at column");

    assert_eq!(layer.value(0), "identity");
    assert_eq!(provenance_source.value(0), "tool_verified");
    assert_eq!(provenance_reference.value(0), "audit:task-9");
    assert_eq!(provenance_evidence.value(0), "https://example.test/task-9");
    assert!((confidence.value(0) - 0.41).abs() < f64::EPSILON);
    assert!((importance.value(0) - 0.73).abs() < f64::EPSILON);
    assert_eq!(privacy.value(0), "secret");
    assert_eq!(occurred_at.value(0), "2026-02-18T11:22:33Z");
}

#[tokio::test]
async fn lancedb_reports_unsupported_semantics() {
    let (_tmp, memory) = memory_harness::lancedb_fixture();
    memory_harness::append_test_event(
        &memory,
        "entity-lancedb",
        "forget.target",
        "sensitive value",
        asteroniris::memory::MemoryCategory::Core,
    )
    .await;

    let soft = memory
        .forget_slot(
            "entity-lancedb",
            "forget.target",
            ForgetMode::Soft,
            "integration-soft",
        )
        .await
        .expect("soft forget should complete");
    assert!(soft.applied);

    let soft_slot = memory
        .resolve_slot("entity-lancedb", "forget.target")
        .await
        .expect("resolve after soft forget")
        .expect("slot should remain as marker rewrite");
    assert_eq!(soft_slot.value, "__LANCEDB_DEGRADED_SOFT_FORGET_MARKER__");
    assert_eq!(soft_slot.source, MemorySource::System);
    assert!((soft_slot.confidence - 0.0).abs() < f64::EPSILON);

    let tombstone = memory
        .forget_slot(
            "entity-lancedb",
            "forget.target",
            ForgetMode::Tombstone,
            "integration-tombstone",
        )
        .await
        .expect("tombstone forget should complete");
    assert!(tombstone.applied);

    let tombstone_slot = memory
        .resolve_slot("entity-lancedb", "forget.target")
        .await
        .expect("resolve after tombstone")
        .expect("slot should remain as marker rewrite");
    assert_eq!(
        tombstone_slot.value,
        "__LANCEDB_DEGRADED_TOMBSTONE_MARKER__"
    );
    assert_eq!(tombstone_slot.source, MemorySource::System);
    assert!((tombstone_slot.importance - 0.0).abs() < f64::EPSILON);
}
