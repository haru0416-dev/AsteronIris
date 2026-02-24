use crate::config::MemoryConfig;
use anyhow::{Context, Result};
use chrono::{Duration, Local};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
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

async fn open_pool(workspace_dir: &Path) -> Result<Option<SqlitePool>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }
    let url = format!("sqlite:{}?mode=rwc", db_path.display());
    let pool = SqlitePool::connect(&url)
        .await
        .context("open memory database for pruning")?;
    Ok(Some(pool))
}

pub(super) async fn prune_conversation_rows(
    workspace_dir: &Path,
    retention_days: u32,
) -> Result<u64> {
    if retention_days == 0 {
        return Ok(0);
    }

    let Some(pool) = open_pool(workspace_dir).await? else {
        return Ok(0);
    };

    let cutoff = (Local::now() - Duration::days(i64::from(retention_days))).to_rfc3339();

    let _ = sqlx::query(
        "DELETE FROM retrieval_units
         WHERE EXISTS (
             SELECT 1
             FROM belief_slots bs
             WHERE bs.entity_id = retrieval_units.entity_id
               AND bs.slot_key = retrieval_units.slot_key
               AND bs.source = 'inferred'
               AND bs.updated_at < ?1
         )",
    )
    .bind(&cutoff)
    .execute(&pool)
    .await;

    let result = sqlx::query(
        "DELETE FROM belief_slots
         WHERE source = 'inferred' AND updated_at < ?1",
    )
    .bind(&cutoff)
    .execute(&pool)
    .await
    .context("delete stale conversation rows")?;

    Ok(result.rows_affected())
}

#[allow(clippy::too_many_lines)]
pub(super) async fn prune_lifecycle_rows(
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

    let Some(pool) = open_pool(workspace_dir).await? else {
        return Ok(LifecyclePruneReport::default());
    };

    let mut report = LifecyclePruneReport::default();
    let now = Local::now().to_rfc3339();
    let ttl_grace_cutoff = (Local::now() - Duration::days(TTL_SOFT_DELETE_GRACE_DAYS)).to_rfc3339();

    // TTL soft-delete
    let _ = sqlx::query(
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
    )
    .bind(&now)
    .execute(&pool)
    .await;

    // TTL hard-delete after grace
    let ttl_hard = sqlx::query(
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
    )
    .bind(&now)
    .bind(&ttl_grace_cutoff)
    .execute(&pool)
    .await;
    report.ttl_slot_hard_deleted = ttl_hard.map(|r| r.rows_affected()).unwrap_or(0);

    // TTL unit purge
    let ttl_unit = sqlx::query(
        "DELETE FROM retrieval_units
         WHERE retention_expires_at IS NOT NULL
           AND retention_expires_at <= ?1",
    )
    .bind(&ttl_grace_cutoff)
    .execute(&pool)
    .await;
    report.ttl_unit_purged = ttl_unit.map(|r| r.rows_affected()).unwrap_or(0);

    // Low-confidence demotion
    let low_conf = sqlx::query(
        "UPDATE retrieval_units
         SET promotion_status = 'demoted', updated_at = ?1
         WHERE promotion_status = 'raw'
           AND signal_tier != 'governance'
           AND signal_tier = 'raw'
           AND reliability < ?2",
    )
    .bind(&now)
    .bind(LOW_CONFIDENCE_RELIABILITY_THRESHOLD)
    .execute(&pool)
    .await;
    report.low_confidence_demoted = low_conf.map(|r| r.rows_affected()).unwrap_or(0);

    // Contradiction auto-demotion
    let contra = sqlx::query(
        "UPDATE retrieval_units
         SET promotion_status = 'demoted', updated_at = ?1
         WHERE promotion_status IN ('promoted', 'candidate')
           AND signal_tier != 'governance'
           AND contradiction_penalty > ?2",
    )
    .bind(&now)
    .bind(HIGH_CONTRADICTION_PENALTY_THRESHOLD)
    .execute(&pool)
    .await;
    report.contradiction_auto_demoted = contra.map(|r| r.rows_affected()).unwrap_or(0);

    // Recency refresh
    let recency = sqlx::query(
        "UPDATE retrieval_units
         SET recency_score = MAX(0.20, 1.0 - ((julianday(?1) - julianday(updated_at)) / 90.0))
         WHERE updated_at IS NOT NULL",
    )
    .bind(&now)
    .execute(&pool)
    .await;
    report.recency_refreshed = recency.map(|r| r.rows_affected()).unwrap_or(0);

    // Stale trend demotion
    let stale_trend = sqlx::query(
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
    )
    .bind(&now)
    .bind(STALE_TREND_DAYS)
    .execute(&pool)
    .await;
    report.stale_trend_demoted = stale_trend.map(|r| r.rows_affected()).unwrap_or(0);

    // Per-layer cleanup
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

        let hard_deleted = sqlx::query(
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
        )
        .bind(&cutoff)
        .bind(layer)
        .execute(&pool)
        .await;
        report.layer_cleanup_actions += hard_deleted.map(|r| r.rows_affected()).unwrap_or(0);

        let tombstone_purge = sqlx::query(
            "DELETE FROM belief_slots
             WHERE status = 'tombstoned'
               AND updated_at < ?1
               AND EXISTS (
                    SELECT 1 FROM retrieval_units
                    WHERE retrieval_units.entity_id = belief_slots.entity_id
                      AND retrieval_units.slot_key = belief_slots.slot_key
                      AND retrieval_units.layer = ?2
                )",
        )
        .bind(&cutoff)
        .bind(layer)
        .execute(&pool)
        .await;
        report.layer_cleanup_actions += tombstone_purge.map(|r| r.rows_affected()).unwrap_or(0);

        let hidden_docs = sqlx::query(
            "DELETE FROM retrieval_units
             WHERE visibility = 'secret'
               AND layer = ?2
               AND updated_at < ?1",
        )
        .bind(&cutoff)
        .bind(layer)
        .execute(&pool)
        .await;
        report.layer_cleanup_actions += hidden_docs.map(|r| r.rows_affected()).unwrap_or(0);
    }

    // Ledger purge
    let ledger_cutoff = (Local::now() - Duration::days(i64::from(ledger_retention))).to_rfc3339();
    let old_ledger = sqlx::query("DELETE FROM deletion_ledger WHERE executed_at < ?1")
        .bind(&ledger_cutoff)
        .execute(&pool)
        .await;
    report.ledger_purged = old_ledger.map(|r| r.rows_affected()).unwrap_or(0);

    Ok(report)
}
