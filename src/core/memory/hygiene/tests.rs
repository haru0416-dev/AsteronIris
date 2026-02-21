use super::state::run_if_due;
use crate::config::MemoryConfig;
use crate::core::memory::traits::MemoryLayer;
use crate::core::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
};
use chrono::{Duration, Local};
use rusqlite::{Connection, params};
use std::fs;
use tempfile::TempDir;

fn default_cfg() -> MemoryConfig {
    MemoryConfig::default()
}

#[test]
fn archives_old_daily_memory_files() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();
    fs::create_dir_all(workspace.join("memory")).unwrap();

    let old = (Local::now().date_naive() - Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();

    let old_file = workspace.join("memory").join(format!("{old}.md"));
    let today_file = workspace.join("memory").join(format!("{today}.md"));
    fs::write(&old_file, "old note").unwrap();
    fs::write(&today_file, "fresh note").unwrap();

    run_if_due(&default_cfg(), workspace).unwrap();

    assert!(!old_file.exists(), "old daily file should be archived");
    assert!(
        workspace
            .join("memory")
            .join("archive")
            .join(format!("{old}.md"))
            .exists(),
        "old daily file should exist in memory/archive"
    );
    assert!(today_file.exists(), "today file should remain in place");
}

#[test]
fn archives_old_session_files() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();
    fs::create_dir_all(workspace.join("sessions")).unwrap();

    let old = (Local::now().date_naive() - Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let old_name = format!("{old}-agent.log");
    let old_file = workspace.join("sessions").join(&old_name);
    fs::write(&old_file, "old session").unwrap();

    run_if_due(&default_cfg(), workspace).unwrap();

    assert!(!old_file.exists(), "old session file should be archived");
    assert!(
        workspace
            .join("sessions")
            .join("archive")
            .join(&old_name)
            .exists(),
        "archived session file should exist"
    );
}

#[test]
fn skips_second_run_within_cadence_window() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();
    fs::create_dir_all(workspace.join("memory")).unwrap();

    let old_a = (Local::now().date_naive() - Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();
    let file_a = workspace.join("memory").join(format!("{old_a}.md"));
    fs::write(&file_a, "first").unwrap();

    run_if_due(&default_cfg(), workspace).unwrap();
    assert!(!file_a.exists(), "first old file should be archived");

    let old_b = (Local::now().date_naive() - Duration::days(9))
        .format("%Y-%m-%d")
        .to_string();
    let file_b = workspace.join("memory").join(format!("{old_b}.md"));
    fs::write(&file_b, "second").unwrap();

    // Should skip because cadence gate prevents a second immediate run.
    run_if_due(&default_cfg(), workspace).unwrap();
    assert!(
        file_b.exists(),
        "second file should remain because run is throttled"
    );
}

#[test]
fn purges_old_memory_archives() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();
    let archive_dir = workspace.join("memory").join("archive");
    fs::create_dir_all(&archive_dir).unwrap();

    let old = (Local::now().date_naive() - Duration::days(40))
        .format("%Y-%m-%d")
        .to_string();
    let keep = (Local::now().date_naive() - Duration::days(5))
        .format("%Y-%m-%d")
        .to_string();

    let old_file = archive_dir.join(format!("{old}.md"));
    let keep_file = archive_dir.join(format!("{keep}.md"));
    fs::write(&old_file, "expired").unwrap();
    fs::write(&keep_file, "recent").unwrap();

    run_if_due(&default_cfg(), workspace).unwrap();

    assert!(!old_file.exists(), "old archived file should be purged");
    assert!(keep_file.exists(), "recent archived file should remain");
}

#[tokio::test]
async fn prunes_old_conversation_rows_in_sqlite_backend() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let mem = SqliteMemory::new(workspace).unwrap();
    mem.append_event(MemoryEventInput::new(
        "default",
        "conv_old",
        MemoryEventType::FactAdded,
        "outdated",
        MemorySource::Inferred,
        PrivacyLevel::Private,
    ))
    .await
    .unwrap();
    mem.append_event(MemoryEventInput::new(
        "default",
        "core_keep",
        MemoryEventType::FactAdded,
        "durable",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    ))
    .await
    .unwrap();
    drop(mem);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let old_cutoff = (Local::now() - Duration::days(60)).to_rfc3339();
    conn.execute(
        "UPDATE memories SET created_at = ?1, updated_at = ?1 WHERE key = 'conv_old'",
        params![old_cutoff],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 30;

    run_if_due(&cfg, workspace).unwrap();

    let mem2 = SqliteMemory::new(workspace).unwrap();
    assert!(
        mem2.resolve_slot("default", "conv_old")
            .await
            .unwrap()
            .is_none(),
        "old conversation rows should be pruned"
    );
    assert!(
        mem2.resolve_slot("default", "core_keep")
            .await
            .unwrap()
            .is_some(),
        "core memory should remain"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn memory_hygiene_per_layer_retention() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    let old = (Local::now() - Duration::days(40)).to_rfc3339();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "working_slot",
                MemoryEventType::FactAdded,
                "working retention case",
                MemorySource::Inferred,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working)
            .with_occurred_at(old.clone()),
        )
        .await
        .unwrap();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "semantic_slot",
                MemoryEventType::FactAdded,
                "semantic retention case",
                MemorySource::Inferred,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Semantic)
            .with_occurred_at(old.clone()),
        )
        .await
        .unwrap();

    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "UPDATE belief_slots SET status = 'soft_deleted', updated_at = ?1 WHERE entity_id = ?2 AND slot_key = ?3",
        params![old, "default", "working_slot"],
    )
    .unwrap();
    conn.execute(
        "UPDATE belief_slots SET status = 'soft_deleted', updated_at = ?1 WHERE entity_id = ?2 AND slot_key = ?3",
        params![old, "default", "semantic_slot"],
    )
    .unwrap();
    conn.execute(
        "UPDATE retrieval_docs SET visibility = 'secret', updated_at = ?1 WHERE doc_id = ?2",
        params![old, "default:working_slot"],
    )
    .unwrap();
    conn.execute(
        "UPDATE retrieval_docs SET visibility = 'secret', updated_at = ?1 WHERE doc_id = ?2",
        params![old, "default:semantic_slot"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO deletion_ledger (
            ledger_id, entity_id, target_slot_key, phase, reason, requested_by, executed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            "ledger-old-1",
            "default",
            "working_slot",
            "soft",
            "test",
            "test",
            old,
        ],
    )
    .unwrap();

    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    cfg.layer_retention_working_days = Some(1);
    cfg.layer_retention_semantic_days = Some(365);
    cfg.layer_retention_episodic_days = Some(1);
    cfg.layer_retention_procedural_days = Some(1);
    cfg.layer_retention_identity_days = Some(1);
    cfg.ledger_retention_days = Some(1);

    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let working_status: String = conn
        .query_row(
            "SELECT status FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
            params!["default", "working_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        working_status, "hard_deleted",
        "working layer should hard-delete stale soft_deleted slots"
    );

    let semantic_status: String = conn
        .query_row(
            "SELECT status FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
            params!["default", "semantic_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        semantic_status, "soft_deleted",
        "semantic layer should be retained with longer policy"
    );

    let working_docs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retrieval_docs WHERE doc_id = ?1",
            params!["default:working_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        working_docs, 0,
        "working retrieval docs should be pruned by secret visibility"
    );

    let semantic_docs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retrieval_docs WHERE doc_id = ?1",
            params!["default:semantic_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        semantic_docs, 1,
        "semantic retrieval docs should persist with longer retention"
    );

    let ledger_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM deletion_ledger", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        ledger_rows, 0,
        "deletion ledger uses separate retention policy"
    );
}
