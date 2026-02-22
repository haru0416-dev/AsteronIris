use super::state::run_if_due;
use crate::config::MemoryConfig;
use crate::core::memory::traits::MemoryLayer;
use crate::core::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SignalTier, SqliteMemory,
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
        "UPDATE belief_slots
         SET updated_at = ?1
         WHERE entity_id = ?2 AND slot_key = ?3",
        params![old_cutoff, "default", "conv_old"],
    )
    .unwrap();
    conn.execute(
        "UPDATE retrieval_units
         SET updated_at = ?1
         WHERE unit_id = ?2",
        params![old_cutoff, "default:conv_old"],
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
        "UPDATE retrieval_units SET visibility = 'secret', updated_at = ?1 WHERE unit_id = ?2",
        params![old, "default:working_slot"],
    )
    .unwrap();
    conn.execute(
        "UPDATE retrieval_units SET visibility = 'secret', updated_at = ?1 WHERE unit_id = ?2",
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
            "SELECT COUNT(*) FROM retrieval_units WHERE unit_id = ?1",
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
            "SELECT COUNT(*) FROM retrieval_units WHERE unit_id = ?1",
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

#[tokio::test]
async fn memory_hygiene_applies_ttl_soft_delete_then_grace_purge() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "ttl_slot",
                MemoryEventType::FactAdded,
                "ttl payload",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let expired = (Local::now() - Duration::days(1)).to_rfc3339();
    conn.execute(
        "UPDATE retrieval_units SET retention_expires_at = ?1 WHERE unit_id = ?2",
        params![expired, "default:ttl_slot"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let units_after_first_tick: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retrieval_units WHERE unit_id = ?1",
            params!["default:ttl_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        units_after_first_tick, 1,
        "freshly expired retrieval unit should be retained during grace window"
    );

    let slot_status_after_first_tick: String = conn
        .query_row(
            "SELECT status FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
            params!["default", "ttl_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        slot_status_after_first_tick, "soft_deleted",
        "ttl should soft-delete belief slot before grace purge"
    );

    let past_grace = (Local::now() - Duration::days(10)).to_rfc3339();
    conn.execute(
        "UPDATE belief_slots SET updated_at = ?1 WHERE entity_id = ?2 AND slot_key = ?3",
        params![past_grace, "default", "ttl_slot"],
    )
    .unwrap();
    conn.execute(
        "UPDATE retrieval_units SET retention_expires_at = ?1 WHERE unit_id = ?2",
        params![past_grace, "default:ttl_slot"],
    )
    .unwrap();
    drop(conn);

    std::fs::remove_file(workspace.join("state").join("memory_hygiene_state.json")).unwrap();

    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let units_after_second_tick: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM retrieval_units WHERE unit_id = ?1",
            params!["default:ttl_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        units_after_second_tick, 0,
        "ttl unit should be purged after grace window"
    );

    let slot_status_after_second_tick: String = conn
        .query_row(
            "SELECT status FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
            params!["default", "ttl_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        slot_status_after_second_tick, "hard_deleted",
        "ttl soft-deleted slot should hard-delete after grace window"
    );
}

#[tokio::test]
async fn memory_hygiene_demotes_low_confidence_promoted_units() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "low_conf_slot",
                MemoryEventType::FactAdded,
                "weak claim",
                MemorySource::Inferred,
                PrivacyLevel::Private,
            )
            .with_signal_tier(SignalTier::Raw)
            .with_confidence(0.2)
            .with_importance(0.9)
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:low_conf_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "demoted",
        "low-confidence raw unit should be demoted"
    );
}

#[tokio::test]
async fn memory_hygiene_does_not_low_confidence_demote_non_raw_units() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "low_conf_non_raw_slot",
                MemoryEventType::FactAdded,
                "non raw weak claim",
                MemorySource::Inferred,
                PrivacyLevel::Private,
            )
            .with_signal_tier(SignalTier::Belief)
            .with_confidence(0.2)
            .with_importance(0.9)
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:low_conf_non_raw_slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "promoted",
        "low-confidence demotion must be restricted to raw signals"
    );
}

#[tokio::test]
async fn memory_hygiene_demotes_stale_trend_units() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "trend.topic.rust",
                MemoryEventType::FactAdded,
                "rust trend up",
                MemorySource::ExternalSecondary,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let old = (Local::now() - Duration::days(30)).to_rfc3339();
    conn.execute(
        "UPDATE retrieval_units SET updated_at = ?1 WHERE unit_id = ?2",
        params![old, "default:trend.topic.rust"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:trend.topic.rust"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "candidate", "stale trend should be demoted");
}

#[tokio::test]
async fn memory_hygiene_does_not_demote_recent_trend_units_within_30_day_window() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "trend.topic.recent",
                MemoryEventType::FactAdded,
                "recent trend pulse",
                MemorySource::ExternalSecondary,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let recent = (Local::now() - Duration::days(20)).to_rfc3339();
    conn.execute(
        "UPDATE retrieval_units SET updated_at = ?1 WHERE unit_id = ?2",
        params![recent, "default:trend.topic.recent"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:trend.topic.recent"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "promoted",
        "trend units newer than 30 days should remain promoted"
    );
}

#[tokio::test]
async fn memory_hygiene_does_not_demote_stale_governance_trend_units() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "trend.topic.policy",
                MemoryEventType::FactAdded,
                "policy trend",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_signal_tier(SignalTier::Governance)
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let old = (Local::now() - Duration::days(30)).to_rfc3339();
    conn.execute(
        "UPDATE retrieval_units SET updated_at = ?1 WHERE unit_id = ?2",
        params![old, "default:trend.topic.policy"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:trend.topic.policy"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "promoted",
        "governance trend should not be stale-demoted"
    );
}

#[tokio::test]
async fn memory_hygiene_auto_demotes_high_contradiction_units() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "contradicted.slot",
                MemoryEventType::FactAdded,
                "contradicted value",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "UPDATE retrieval_units SET contradiction_penalty = 0.75 WHERE unit_id = ?1",
        params!["default:contradicted.slot"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:contradicted.slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "demoted", "high contradiction should auto-demote");
}

#[tokio::test]
async fn memory_hygiene_does_not_auto_demote_at_contradiction_threshold() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "contradicted.threshold.slot",
                MemoryEventType::FactAdded,
                "threshold contradiction value",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "UPDATE retrieval_units SET contradiction_penalty = 0.5 WHERE unit_id = ?1",
        params!["default:contradicted.threshold.slot"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let status: String = conn
        .query_row(
            "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
            params!["default:contradicted.threshold.slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "promoted",
        "contradiction threshold boundary should not auto-demote"
    );
}

#[tokio::test]
async fn memory_hygiene_refreshes_recency_score_from_age() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "recency.slot",
                MemoryEventType::FactAdded,
                "recency value",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();
    drop(memory);

    let db_path = workspace.join("memory").join("brain.db");
    let conn = Connection::open(&db_path).unwrap();
    let old = (Local::now() - Duration::days(45)).to_rfc3339();
    conn.execute(
        "UPDATE retrieval_units SET updated_at = ?1, recency_score = 1.0 WHERE unit_id = ?2",
        params![old, "default:recency.slot"],
    )
    .unwrap();
    drop(conn);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due(&cfg, workspace).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let recency: f64 = conn
        .query_row(
            "SELECT recency_score FROM retrieval_units WHERE unit_id = ?1",
            params!["default:recency.slot"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(recency < 0.7, "recency should decay for old entries");
    assert!(recency >= 0.2, "recency should respect floor");
}
