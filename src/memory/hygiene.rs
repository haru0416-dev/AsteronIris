use crate::config::MemoryConfig;
use anyhow::Result;
use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, SystemTime};

const HYGIENE_INTERVAL_HOURS: i64 = 12;
const STATE_FILE: &str = "memory_hygiene_state.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HygieneReport {
    archived_memory_files: u64,
    archived_session_files: u64,
    purged_memory_archives: u64,
    purged_session_archives: u64,
    pruned_conversation_rows: u64,
}

impl HygieneReport {
    fn total_actions(&self) -> u64 {
        self.archived_memory_files
            + self.archived_session_files
            + self.purged_memory_archives
            + self.purged_session_archives
            + self.pruned_conversation_rows
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HygieneState {
    last_run_at: Option<String>,
    last_report: HygieneReport,
}

/// Run memory/session hygiene if the cadence window has elapsed.
///
/// This function is intentionally best-effort: callers should log and continue on failure.
pub fn run_if_due(config: &MemoryConfig, workspace_dir: &Path) -> Result<()> {
    if !config.hygiene_enabled {
        return Ok(());
    }

    if !should_run_now(workspace_dir)? {
        return Ok(());
    }

    let report = HygieneReport {
        archived_memory_files: archive_daily_memory_files(
            workspace_dir,
            config.archive_after_days,
        )?,
        archived_session_files: archive_session_files(workspace_dir, config.archive_after_days)?,
        purged_memory_archives: purge_memory_archives(workspace_dir, config.purge_after_days)?,
        purged_session_archives: purge_session_archives(workspace_dir, config.purge_after_days)?,
        pruned_conversation_rows: prune_conversation_rows(
            workspace_dir,
            config.conversation_retention_days,
        )?,
    };

    let _ = prune_v2_lifecycle_rows(workspace_dir, config)?;

    write_state(workspace_dir, &report)?;

    if report.total_actions() > 0 {
        tracing::info!(
            "memory hygiene complete: archived_memory={} archived_sessions={} purged_memory={} purged_sessions={} pruned_conversation_rows={}",
            report.archived_memory_files,
            report.archived_session_files,
            report.purged_memory_archives,
            report.purged_session_archives,
            report.pruned_conversation_rows,
        );
    }

    Ok(())
}

fn should_run_now(workspace_dir: &Path) -> Result<bool> {
    let path = state_path(workspace_dir);
    if !path.exists() {
        return Ok(true);
    }

    let raw = fs::read_to_string(&path)?;
    let state: HygieneState = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(_) => return Ok(true),
    };

    let Some(last_run_at) = state.last_run_at else {
        return Ok(true);
    };

    let last = match DateTime::parse_from_rfc3339(&last_run_at) {
        Ok(ts) => ts.with_timezone(&Utc),
        Err(_) => return Ok(true),
    };

    Ok(Utc::now().signed_duration_since(last) >= Duration::hours(HYGIENE_INTERVAL_HOURS))
}

fn write_state(workspace_dir: &Path, report: &HygieneReport) -> Result<()> {
    let path = state_path(workspace_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let state = HygieneState {
        last_run_at: Some(Utc::now().to_rfc3339()),
        last_report: report.clone(),
    };
    let json = serde_json::to_vec_pretty(&state)?;
    fs::write(path, json)?;
    Ok(())
}

fn state_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("state").join(STATE_FILE)
}

fn archive_daily_memory_files(workspace_dir: &Path, archive_after_days: u32) -> Result<u64> {
    if archive_after_days == 0 {
        return Ok(0);
    }

    let memory_dir = workspace_dir.join("memory");
    if !memory_dir.is_dir() {
        return Ok(0);
    }

    let archive_dir = memory_dir.join("archive");
    fs::create_dir_all(&archive_dir)?;

    let cutoff = Local::now().date_naive() - Duration::days(i64::from(archive_after_days));
    let mut moved = 0_u64;

    for entry in fs::read_dir(&memory_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };

        let Some(file_date) = memory_date_from_filename(filename) else {
            continue;
        };

        if file_date < cutoff {
            move_to_archive(&path, &archive_dir)?;
            moved += 1;
        }
    }

    Ok(moved)
}

fn archive_session_files(workspace_dir: &Path, archive_after_days: u32) -> Result<u64> {
    if archive_after_days == 0 {
        return Ok(0);
    }

    let sessions_dir = workspace_dir.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(0);
    }

    let archive_dir = sessions_dir.join("archive");
    fs::create_dir_all(&archive_dir)?;

    let cutoff_date = Local::now().date_naive() - Duration::days(i64::from(archive_after_days));
    let cutoff_time = SystemTime::now()
        .checked_sub(StdDuration::from_secs(
            u64::from(archive_after_days) * 24 * 60 * 60,
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut moved = 0_u64;
    for entry in fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };

        let is_old = if let Some(date) = date_prefix(filename) {
            date < cutoff_date
        } else {
            is_older_than(&path, cutoff_time)
        };

        if is_old {
            move_to_archive(&path, &archive_dir)?;
            moved += 1;
        }
    }

    Ok(moved)
}

fn purge_memory_archives(workspace_dir: &Path, purge_after_days: u32) -> Result<u64> {
    if purge_after_days == 0 {
        return Ok(0);
    }

    let archive_dir = workspace_dir.join("memory").join("archive");
    if !archive_dir.is_dir() {
        return Ok(0);
    }

    let cutoff = Local::now().date_naive() - Duration::days(i64::from(purge_after_days));
    let mut removed = 0_u64;

    for entry in fs::read_dir(&archive_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };

        let Some(file_date) = memory_date_from_filename(filename) else {
            continue;
        };

        if file_date < cutoff {
            fs::remove_file(&path)?;
            removed += 1;
        }
    }

    Ok(removed)
}

fn purge_session_archives(workspace_dir: &Path, purge_after_days: u32) -> Result<u64> {
    if purge_after_days == 0 {
        return Ok(0);
    }

    let archive_dir = workspace_dir.join("sessions").join("archive");
    if !archive_dir.is_dir() {
        return Ok(0);
    }

    let cutoff_date = Local::now().date_naive() - Duration::days(i64::from(purge_after_days));
    let cutoff_time = SystemTime::now()
        .checked_sub(StdDuration::from_secs(
            u64::from(purge_after_days) * 24 * 60 * 60,
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut removed = 0_u64;
    for entry in fs::read_dir(&archive_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            continue;
        }

        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };

        let is_old = if let Some(date) = date_prefix(filename) {
            date < cutoff_date
        } else {
            is_older_than(&path, cutoff_time)
        };

        if is_old {
            fs::remove_file(&path)?;
            removed += 1;
        }
    }

    Ok(removed)
}

fn prune_conversation_rows(workspace_dir: &Path, retention_days: u32) -> Result<u64> {
    if retention_days == 0 {
        return Ok(0);
    }

    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = Connection::open(db_path)?;
    let cutoff = (Local::now() - Duration::days(i64::from(retention_days))).to_rfc3339();

    let mut stale_keys = Vec::new();
    let mut key_stmt = conn
        .prepare("SELECT key FROM memories WHERE category = 'conversation' AND updated_at < ?1")?;
    let key_rows = key_stmt.query_map(params![cutoff], |row| row.get::<_, String>(0))?;
    for key_row in key_rows {
        stale_keys.push(key_row?);
    }
    drop(key_stmt);

    let affected = conn.execute(
        "DELETE FROM memories WHERE category = 'conversation' AND updated_at < ?1",
        params![cutoff],
    )?;

    for key in &stale_keys {
        if let Some((entity_id, slot_key)) = key.split_once(':') {
            let _ = conn.execute(
                "DELETE FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
                params![entity_id, slot_key],
            );
            let _ = conn.execute(
                "DELETE FROM retrieval_docs WHERE entity_id = ?1 AND slot_key = ?2",
                params![entity_id, slot_key],
            );
        } else {
            let _ = conn.execute(
                "DELETE FROM belief_slots WHERE entity_id = 'default' AND slot_key = ?1",
                params![key],
            );
            let _ = conn.execute(
                "DELETE FROM retrieval_docs WHERE doc_id = ?1 OR slot_key = ?1",
                params![key],
            );
        }
    }

    Ok(u64::try_from(affected).unwrap_or(0))
}

#[allow(clippy::too_many_lines)]
fn prune_v2_lifecycle_rows(workspace_dir: &Path, config: &MemoryConfig) -> Result<u64> {
    let working_retention = config.layer_retention_days("working");
    let episodic_retention = config.layer_retention_days("episodic");
    let semantic_retention = config.layer_retention_days("semantic");
    let procedural_retention = config.layer_retention_days("procedural");
    let identity_retention = config.layer_retention_days("identity");
    let ledger_retention = config.ledger_retention_or_default();

    if working_retention == 0
        && episodic_retention == 0
        && semantic_retention == 0
        && procedural_retention == 0
        && identity_retention == 0
        && ledger_retention == 0
    {
        return Ok(0);
    }

    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = Connection::open(db_path)?;
    let mut affected_total = 0_u64;

    let layer_purge_ops = [
        ("working", working_retention),
        ("episodic", episodic_retention),
        ("semantic", semantic_retention),
        ("procedural", procedural_retention),
        ("identity", identity_retention),
    ];

    for (layer, retention_days) in layer_purge_ops.iter().copied() {
        if retention_days == 0 {
            continue;
        }

        let cutoff = (Local::now() - Duration::days(i64::from(retention_days))).to_rfc3339();

        let hard_deleted = match conn.execute(
            "UPDATE belief_slots
             SET status = 'hard_deleted', updated_at = ?1
             WHERE status = 'soft_deleted'
               AND updated_at < ?1
               AND EXISTS (
                   SELECT 1 FROM memories
                   WHERE memories.key = belief_slots.slot_key
                     AND memories.layer = ?2
               )",
            params![cutoff, layer],
        ) {
            Ok(n) => n,
            Err(rusqlite::Error::SqliteFailure(_, Some(message)))
                if message.contains("no such table") =>
            {
                0
            }
            Err(err) => return Err(err.into()),
        };
        affected_total += u64::try_from(hard_deleted).unwrap_or(0);

        let tombstone_purge = match conn.execute(
            "DELETE FROM belief_slots
             WHERE status = 'tombstoned'
               AND updated_at < ?1
               AND EXISTS (
                   SELECT 1 FROM memories
                   WHERE memories.key = belief_slots.slot_key
                     AND memories.layer = ?2
               )",
            params![cutoff, layer],
        ) {
            Ok(n) => n,
            Err(rusqlite::Error::SqliteFailure(_, Some(message)))
                if message.contains("no such table") =>
            {
                0
            }
            Err(err) => return Err(err.into()),
        };
        affected_total += u64::try_from(tombstone_purge).unwrap_or(0);

        let hidden_docs = match conn.execute(
            "DELETE FROM retrieval_docs
             WHERE visibility = 'secret'
               AND layer = ?2
               AND updated_at < ?1",
            params![cutoff, layer],
        ) {
            Ok(n) => n,
            Err(rusqlite::Error::SqliteFailure(_, Some(message)))
                if message.contains("no such table") =>
            {
                0
            }
            Err(err) => return Err(err.into()),
        };
        affected_total += u64::try_from(hidden_docs).unwrap_or(0);
    }

    let ledger_cutoff = (Local::now() - Duration::days(i64::from(ledger_retention))).to_rfc3339();
    let old_ledger = match conn.execute(
        "DELETE FROM deletion_ledger WHERE executed_at < ?1",
        params![ledger_cutoff],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    affected_total += u64::try_from(old_ledger).unwrap_or(0);

    Ok(affected_total)
}

fn memory_date_from_filename(filename: &str) -> Option<NaiveDate> {
    let stem = filename.strip_suffix(".md")?;
    let date_part = stem.split('_').next().unwrap_or(stem);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

fn date_prefix(filename: &str) -> Option<NaiveDate> {
    if filename.len() < 10 {
        return None;
    }
    NaiveDate::parse_from_str(&filename[..filename.floor_char_boundary(10)], "%Y-%m-%d").ok()
}

fn is_older_than(path: &Path, cutoff: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|modified| modified < cutoff)
        .unwrap_or(false)
}

fn move_to_archive(src: &Path, archive_dir: &Path) -> Result<()> {
    let Some(filename) = src.file_name().and_then(|f| f.to_str()) else {
        return Ok(());
    };

    let target = unique_archive_target(archive_dir, filename);
    fs::rename(src, target)?;
    Ok(())
}

fn unique_archive_target(archive_dir: &Path, filename: &str) -> PathBuf {
    let direct = archive_dir.join(filename);
    if !direct.exists() {
        return direct;
    }

    let (stem, ext) = split_name(filename);
    for i in 1..10_000 {
        let candidate = if ext.is_empty() {
            archive_dir.join(format!("{stem}_{i}"))
        } else {
            archive_dir.join(format!("{stem}_{i}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }

    direct
}

fn split_name(filename: &str) -> (&str, &str) {
    match filename.rsplit_once('.') {
        Some((stem, ext)) => (stem, ext),
        None => (filename, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::traits::MemoryLayer;
    use crate::memory::{
        Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
    };
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
}
