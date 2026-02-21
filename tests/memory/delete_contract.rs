use asteroniris::core::memory::{
    ForgetArtifact, ForgetArtifactRequirement, ForgetMode, ForgetStatus, Memory,
};
use rusqlite::Connection;

use super::memory_harness;

fn check_for_artifact(
    outcome: &asteroniris::core::memory::ForgetOutcome,
    artifact: ForgetArtifact,
) -> &asteroniris::core::memory::ForgetArtifactCheck {
    outcome
        .artifact_checks
        .iter()
        .find(|check| check.artifact == artifact)
        .expect("artifact check should be present")
}

#[tokio::test]
async fn memory_delete_contract_artifact_matrix() {
    let (_tmp, memory) = memory_harness::sqlite_fixture();

    memory_harness::append_test_event(
        &memory,
        "entity-delete",
        "slot.matrix",
        "highly sensitive value",
        asteroniris::core::memory::MemoryCategory::Core,
    )
    .await;

    let soft = memory
        .forget_slot(
            "entity-delete",
            "slot.matrix",
            ForgetMode::Soft,
            "artifact-soft",
        )
        .await
        .expect("soft forget should execute");
    assert!(soft.applied);
    assert!(soft.complete);
    assert!(!soft.degraded);
    assert_eq!(soft.status, ForgetStatus::Complete);
    assert_eq!(
        check_for_artifact(&soft, ForgetArtifact::Slot).requirement,
        ForgetArtifactRequirement::MustBeNonRetrievable
    );
    assert_eq!(
        check_for_artifact(&soft, ForgetArtifact::RetrievalDocs).requirement,
        ForgetArtifactRequirement::MustBeNonRetrievable
    );
    assert!(check_for_artifact(&soft, ForgetArtifact::Slot).satisfied);
    assert!(check_for_artifact(&soft, ForgetArtifact::RetrievalDocs).satisfied);

    memory_harness::append_test_event(
        &memory,
        "entity-delete",
        "slot.matrix",
        "highly sensitive value",
        asteroniris::core::memory::MemoryCategory::Core,
    )
    .await;

    let hard = memory
        .forget_slot(
            "entity-delete",
            "slot.matrix",
            ForgetMode::Hard,
            "artifact-hard",
        )
        .await
        .expect("hard forget should execute");
    assert!(hard.applied);
    assert!(hard.complete);
    assert!(!hard.degraded);
    assert_eq!(hard.status, ForgetStatus::Complete);
    assert!(
        hard.artifact_checks.iter().all(|check| check.satisfied),
        "hard delete contract should satisfy all governed artifacts"
    );

    memory_harness::append_test_event(
        &memory,
        "entity-delete",
        "slot.matrix",
        "highly sensitive value",
        asteroniris::core::memory::MemoryCategory::Core,
    )
    .await;

    let tombstone = memory
        .forget_slot(
            "entity-delete",
            "slot.matrix",
            ForgetMode::Tombstone,
            "artifact-tombstone",
        )
        .await
        .expect("tombstone forget should execute");
    assert!(tombstone.applied);
    assert!(tombstone.complete);
    assert!(!tombstone.degraded);
    assert_eq!(tombstone.status, ForgetStatus::Complete);
    assert!(check_for_artifact(&tombstone, ForgetArtifact::Slot).satisfied);
    assert!(check_for_artifact(&tombstone, ForgetArtifact::RetrievalDocs).satisfied);
}

#[tokio::test]
async fn memory_delete_contract_degraded_backend() {
    let (_tmp_lancedb, lancedb) = memory_harness::lancedb_fixture();
    memory_harness::append_test_event(
        &lancedb,
        "entity-degraded",
        "slot.degraded",
        "value",
        asteroniris::core::memory::MemoryCategory::Core,
    )
    .await;

    let lancedb_soft = lancedb
        .forget_slot(
            "entity-degraded",
            "slot.degraded",
            ForgetMode::Soft,
            "degraded-soft",
        )
        .await
        .expect("lancedb soft forget should execute");
    assert!(lancedb_soft.applied);
    assert!(!lancedb_soft.complete);
    assert!(lancedb_soft.degraded);
    assert_eq!(lancedb_soft.status, ForgetStatus::DegradedNonComplete);

    let (_tmp_markdown, markdown) = memory_harness::markdown_fixture();
    memory_harness::append_test_event(
        &markdown,
        "entity-degraded",
        "slot.degraded",
        "value",
        asteroniris::core::memory::MemoryCategory::Core,
    )
    .await;

    let markdown_hard = markdown
        .forget_slot(
            "entity-degraded",
            "slot.degraded",
            ForgetMode::Hard,
            "degraded-hard",
        )
        .await
        .expect("markdown hard forget should return explicit degraded result");
    assert!(!markdown_hard.applied);
    assert!(!markdown_hard.complete);
    assert!(markdown_hard.degraded);
    assert_eq!(markdown_hard.status, ForgetStatus::DegradedNonComplete);
}

#[tokio::test]
async fn memory_delete_contract_sqlite_hard_delete_dsar_authoritative() {
    let (tmp, memory) = memory_harness::sqlite_fixture();
    memory_harness::append_test_event(
        &memory,
        "entity-dsar",
        "pii.email",
        "person@example.test",
        asteroniris::core::memory::MemoryCategory::Core,
    )
    .await;

    let hard = memory
        .forget_slot(
            "entity-dsar",
            "pii.email",
            ForgetMode::Hard,
            "dsar-hard-delete",
        )
        .await
        .expect("sqlite hard delete should execute");

    assert!(hard.applied);
    assert!(hard.complete);
    assert_eq!(hard.status, ForgetStatus::Complete);

    assert!(
        memory
            .resolve_slot("entity-dsar", "pii.email")
            .await
            .expect("resolve should succeed")
            .is_none(),
        "slot should be non-retrievable after hard delete"
    );

    let recall =
        memory_harness::recall_scoped_items(&memory, "entity-dsar", "example.test", 5).await;
    assert!(
        recall.is_empty(),
        "retrieval docs should be non-retrievable after hard delete"
    );

    let conn = Connection::open(tmp.path().join("memory").join("brain.db"))
        .expect("sqlite db should be readable for artifact assertions");
    let slot_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
            rusqlite::params!["entity-dsar", "pii.email"],
            |row| row.get(0),
        )
        .expect("belief slot count query should succeed");
    let retrieval_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retrieval_docs WHERE doc_id = ?1",
            rusqlite::params!["entity-dsar:pii.email"],
            |row| row.get(0),
        )
        .expect("retrieval docs count query should succeed");
    let projection_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE key = ?1",
            rusqlite::params!["pii.email"],
            |row| row.get(0),
        )
        .expect("projection count query should succeed");
    let ledger_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM deletion_ledger WHERE entity_id = ?1 AND target_slot_key = ?2 AND phase = 'hard'",
            rusqlite::params!["entity-dsar", "pii.email"],
            |row| row.get(0),
        )
        .expect("deletion ledger query should succeed");

    assert_eq!(slot_count, 0, "slot artifact should be deleted");
    assert_eq!(retrieval_count, 0, "retrieval artifact should be deleted");
    assert_eq!(projection_count, 0, "projection artifact should be deleted");
    assert_eq!(
        ledger_count, 1,
        "deletion ledger should capture DSAR delete"
    );
}
