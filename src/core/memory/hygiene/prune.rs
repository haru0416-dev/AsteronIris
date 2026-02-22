use crate::config::MemoryConfig;
use anyhow::{Context, Result};
use chrono::{Duration, Local};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

const LOW_CONFIDENCE_RELIABILITY_THRESHOLD: f64 = 0.30;
const HIGH_CONTRADICTION_PENALTY_THRESHOLD: f64 = 0.50;
const STALE_TREND_DAYS: i64 = 30;
const TTL_SOFT_DELETE_GRACE_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(super) struct LifecyclePruneReport {
    pub(super) ttl_slot_hard_deleted: u64,
    pub(super) ttl_unit_purged: u64,
    pub(super) low_confidence_demoted: u64,
    pub(super) contradiction_auto_demoted: u64,
    pub(super) stale_trend_demoted: u64,
    pub(super) recency_refreshed: u64,
    pub(super) layer_cleanup_actions: u64,
    pub(super) ledger_purged: u64,
}

impl LifecyclePruneReport {
    pub(super) fn total_actions(self) -> u64 {
        self.ttl_slot_hard_deleted
            + self.ttl_unit_purged
            + self.low_confidence_demoted
            + self.contradiction_auto_demoted
            + self.stale_trend_demoted
            + self.recency_refreshed
            + self.layer_cleanup_actions
            + self.ledger_purged
    }
}

pub(super) fn prune_conversation_rows(workspace_dir: &Path, retention_days: u32) -> Result<u64> {
    if retention_days == 0 {
        return Ok(0);
    }

    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = Connection::open(db_path).context("open memory database for pruning")?;
    let cutoff = (Local::now() - Duration::days(i64::from(retention_days))).to_rfc3339();

    let _ = conn.execute(
        "DELETE FROM retrieval_units
         WHERE EXISTS (
             SELECT 1
             FROM belief_slots bs
             WHERE bs.entity_id = retrieval_units.entity_id
               AND bs.slot_key = retrieval_units.slot_key
               AND bs.source = 'inferred'
               AND bs.updated_at < ?1
         )",
        params![cutoff],
    );

    let affected = conn
        .execute(
            "DELETE FROM belief_slots
             WHERE source = 'inferred' AND updated_at < ?1",
            params![cutoff],
        )
        .context("delete stale conversation rows")?;

    Ok(u64::try_from(affected).unwrap_or(0))
}

#[allow(clippy::too_many_lines)]
pub(super) fn prune_v2_lifecycle_rows(
    workspace_dir: &Path,
    config: &MemoryConfig,
) -> Result<LifecyclePruneReport> {
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
        return Ok(LifecyclePruneReport::default());
    }

    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(LifecyclePruneReport::default());
    }

    let conn = Connection::open(db_path).context("open memory database for lifecycle pruning")?;
    let mut report = LifecyclePruneReport::default();
    let now = Local::now().to_rfc3339();
    let ttl_grace_cutoff = (Local::now() - Duration::days(TTL_SOFT_DELETE_GRACE_DAYS)).to_rfc3339();

    let _ttl_slot_soft_deleted = match conn.execute(
        "UPDATE belief_slots
         SET status = 'soft_deleted', updated_at = ?1
         WHERE status NOT IN ('soft_deleted', 'hard_deleted', 'tombstoned')
           AND EXISTS (
               SELECT 1 FROM retrieval_units
               WHERE retrieval_units.entity_id = belief_slots.entity_id
                 AND retrieval_units.slot_key = belief_slots.slot_key
                 AND retrieval_units.retention_expires_at IS NOT NULL
                 AND retrieval_units.retention_expires_at <= ?1
           )",
        params![now],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };

    let ttl_slot_hard_deleted = match conn.execute(
        "UPDATE belief_slots
         SET status = 'hard_deleted', updated_at = ?1
         WHERE status = 'soft_deleted'
           AND updated_at <= ?2
           AND EXISTS (
              SELECT 1 FROM retrieval_units
              WHERE retrieval_units.entity_id = belief_slots.entity_id
                AND retrieval_units.slot_key = belief_slots.slot_key
                AND retrieval_units.retention_expires_at IS NOT NULL
                AND retrieval_units.retention_expires_at <= ?2
          )",
        params![now, ttl_grace_cutoff],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    report.ttl_slot_hard_deleted = u64::try_from(ttl_slot_hard_deleted).unwrap_or(0);

    let ttl_unit_purged = match conn.execute(
        "DELETE FROM retrieval_units
         WHERE retention_expires_at IS NOT NULL
           AND retention_expires_at <= ?1",
        params![ttl_grace_cutoff],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    report.ttl_unit_purged = u64::try_from(ttl_unit_purged).unwrap_or(0);

    let low_confidence_demoted = match conn.execute(
        "UPDATE retrieval_units
         SET promotion_status = 'demoted', updated_at = ?1
         WHERE promotion_status = 'raw'
           AND signal_tier != 'governance'
           AND signal_tier = 'raw'
           AND reliability < ?2",
        params![now, LOW_CONFIDENCE_RELIABILITY_THRESHOLD],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    report.low_confidence_demoted = u64::try_from(low_confidence_demoted).unwrap_or(0);

    let contradiction_auto_demoted = match conn.execute(
        "UPDATE retrieval_units
         SET promotion_status = 'demoted', updated_at = ?1
         WHERE promotion_status IN ('promoted', 'candidate')
           AND signal_tier != 'governance'
           AND contradiction_penalty > ?2",
        params![now, HIGH_CONTRADICTION_PENALTY_THRESHOLD],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    report.contradiction_auto_demoted = u64::try_from(contradiction_auto_demoted).unwrap_or(0);

    let recency_refreshed = match conn.execute(
        "UPDATE retrieval_units
         SET recency_score = MAX(0.20, 1.0 - ((julianday(?1) - julianday(updated_at)) / 90.0))
         WHERE updated_at IS NOT NULL",
        params![now],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    report.recency_refreshed = u64::try_from(recency_refreshed).unwrap_or(0);

    let stale_trend_demoted = match conn.execute(
        "UPDATE retrieval_units
         SET promotion_status = 'candidate', updated_at = ?1
         WHERE promotion_status = 'promoted'
           AND signal_tier != 'governance'
           AND (
             slot_key LIKE 'trend.%'
             OR slot_key LIKE 'trend/%'
             OR slot_key LIKE '%.trend.%'
             OR slot_key LIKE '%/trend/%'
           )
           AND (julianday(?1) - julianday(updated_at)) >= ?2",
        params![now, STALE_TREND_DAYS],
    ) {
        Ok(n) => n,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            0
        }
        Err(err) => return Err(err.into()),
    };
    report.stale_trend_demoted = u64::try_from(stale_trend_demoted).unwrap_or(0);

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
                    SELECT 1 FROM retrieval_units
                    WHERE retrieval_units.entity_id = belief_slots.entity_id
                      AND retrieval_units.slot_key = belief_slots.slot_key
                      AND retrieval_units.layer = ?2
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
        report.layer_cleanup_actions += u64::try_from(hard_deleted).unwrap_or(0);

        let tombstone_purge = match conn.execute(
            "DELETE FROM belief_slots
             WHERE status = 'tombstoned'
               AND updated_at < ?1
               AND EXISTS (
                    SELECT 1 FROM retrieval_units
                    WHERE retrieval_units.entity_id = belief_slots.entity_id
                      AND retrieval_units.slot_key = belief_slots.slot_key
                      AND retrieval_units.layer = ?2
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
        report.layer_cleanup_actions += u64::try_from(tombstone_purge).unwrap_or(0);

        let hidden_docs = match conn.execute(
            "DELETE FROM retrieval_units
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
        report.layer_cleanup_actions += u64::try_from(hidden_docs).unwrap_or(0);
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
    report.ledger_purged = u64::try_from(old_ledger).unwrap_or(0);

    Ok(report)
}
