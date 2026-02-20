use crate::config::MemoryConfig;
use anyhow::Result;
use chrono::{Duration, Local};
use rusqlite::{Connection, params};
use std::path::Path;

pub(super) fn prune_conversation_rows(workspace_dir: &Path, retention_days: u32) -> Result<u64> {
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
pub(super) fn prune_v2_lifecycle_rows(workspace_dir: &Path, config: &MemoryConfig) -> Result<u64> {
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
