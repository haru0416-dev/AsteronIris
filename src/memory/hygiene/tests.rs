use super::state::{run_if_due, run_if_due_async};
use crate::config::MemoryConfig;
use crate::memory::SqliteMemory;
use crate::memory::traits::Memory;
use crate::memory::types::{
    MemoryEventInput, MemoryEventType, MemoryLayer, MemorySource, PrivacyLevel, SignalTier,
};
use chrono::{Duration, Local};
use sqlx::SqlitePool;
use std::fs;
use tempfile::TempDir;

fn default_cfg() -> MemoryConfig {
    MemoryConfig::default()
}

async fn open_test_pool(workspace_dir: &std::path::Path) -> SqlitePool {
    let db_path = workspace_dir.join("memory").join("brain.db");
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    SqlitePool::connect(&url).await.unwrap()
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

    let mem = SqliteMemory::new(workspace).await.unwrap();
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

    let pool = open_test_pool(workspace).await;
    let old_cutoff = (Local::now() - Duration::days(60)).to_rfc3339();
    sqlx::query("UPDATE belief_slots SET updated_at = ?1 WHERE entity_id = ?2 AND slot_key = ?3")
        .bind(&old_cutoff)
        .bind("default")
        .bind("conv_old")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE retrieval_units SET updated_at = ?1 WHERE unit_id = ?2")
        .bind(&old_cutoff)
        .bind("default:conv_old")
        .execute(&pool)
        .await
        .unwrap();
    drop(pool);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 30;

    run_if_due_async(&cfg, workspace).await.unwrap();

    let mem2 = SqliteMemory::new(workspace).await.unwrap();
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
async fn memory_hygiene_demotes_low_confidence_promoted_units() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).await.unwrap();
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
    run_if_due_async(&cfg, workspace).await.unwrap();

    let pool = open_test_pool(workspace).await;
    let status: (String,) =
        sqlx::query_as("SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1")
            .bind("default:low_conf_slot")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        status.0, "demoted",
        "low-confidence raw unit should be demoted"
    );
}

#[tokio::test]
async fn memory_hygiene_refreshes_recency_score_from_age() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path();

    let memory = SqliteMemory::new(workspace).await.unwrap();
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

    let pool = open_test_pool(workspace).await;
    let old = (Local::now() - Duration::days(45)).to_rfc3339();
    sqlx::query(
        "UPDATE retrieval_units SET updated_at = ?1, recency_score = 1.0 WHERE unit_id = ?2",
    )
    .bind(&old)
    .bind("default:recency.slot")
    .execute(&pool)
    .await
    .unwrap();
    drop(pool);

    let mut cfg = default_cfg();
    cfg.archive_after_days = 0;
    cfg.purge_after_days = 0;
    cfg.conversation_retention_days = 365;
    run_if_due_async(&cfg, workspace).await.unwrap();

    let pool = open_test_pool(workspace).await;
    let recency: (f64,) =
        sqlx::query_as("SELECT recency_score FROM retrieval_units WHERE unit_id = ?1")
            .bind("default:recency.slot")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(recency.0 < 0.7, "recency should decay for old entries");
    assert!(recency.0 >= 0.2, "recency should respect floor");
}
