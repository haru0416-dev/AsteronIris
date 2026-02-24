use super::codec;
use crate::memory::types::{
    BeliefSlot, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation, ForgetMode,
    ForgetOutcome,
};
use anyhow::Context;
use chrono::Local;
use sqlx::SqlitePool;
use uuid::Uuid;

/// Resolve the current belief slot for `(entity_id, slot_key)`.
///
/// Returns `None` when the slot does not exist or is not active.
pub(super) async fn resolve_slot(
    pool: &SqlitePool,
    entity_id: &str,
    slot_key: &str,
) -> anyhow::Result<Option<BeliefSlot>> {
    let row: Option<(String, String, f64, f64, String, String)> = sqlx::query_as(
        "SELECT value, source, confidence, importance, privacy_level, updated_at
         FROM belief_slots
         WHERE entity_id = ?1 AND slot_key = ?2 AND status = 'active'",
    )
    .bind(entity_id)
    .bind(slot_key)
    .fetch_optional(pool)
    .await
    .context("query belief slot")?;

    Ok(row.map(
        |(value, source, confidence, importance, privacy_level, updated_at)| BeliefSlot {
            entity_id: entity_id.to_string(),
            slot_key: slot_key.to_string(),
            value,
            source: codec::str_to_source(&source),
            confidence,
            importance,
            privacy_level: codec::str_to_privacy(&privacy_level),
            updated_at,
        },
    ))
}

/// Execute a forget operation on `(entity_id, slot_key)`.
#[allow(clippy::too_many_lines)]
pub(super) async fn forget_slot(
    pool: &SqlitePool,
    entity_id: &str,
    slot_key: &str,
    mode: ForgetMode,
    reason: &str,
) -> anyhow::Result<ForgetOutcome> {
    let now = Local::now().to_rfc3339();
    let phase = match mode {
        ForgetMode::Soft => "soft",
        ForgetMode::Hard => "hard",
        ForgetMode::Tombstone => "tombstone",
    };

    let mut tx = pool.begin().await.context("begin forget transaction")?;

    // Ledger entry
    sqlx::query(
        "INSERT INTO deletion_ledger (
            ledger_id, entity_id, target_slot_key, phase, reason, requested_by, executed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'memory_forget', ?6)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(entity_id)
    .bind(slot_key)
    .bind(phase)
    .bind(reason)
    .bind(&now)
    .execute(&mut *tx)
    .await
    .context("insert deletion ledger entry")?;

    let unit_id = format!("{entity_id}:{slot_key}");
    let applied = match mode {
        ForgetMode::Soft => {
            let result = sqlx::query(
                "UPDATE belief_slots SET status = 'soft_deleted', updated_at = ?3
                 WHERE entity_id = ?1 AND slot_key = ?2",
            )
            .bind(entity_id)
            .bind(slot_key)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .context("soft delete belief slot")?;

            sqlx::query(
                "UPDATE retrieval_units SET visibility = 'secret', updated_at = ?2
                 WHERE unit_id = ?1",
            )
            .bind(&unit_id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .context("hide retrieval unit")?;

            result.rows_affected() > 0
        }
        ForgetMode::Hard => {
            let result =
                sqlx::query("DELETE FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2")
                    .bind(entity_id)
                    .bind(slot_key)
                    .execute(&mut *tx)
                    .await
                    .context("delete belief slot")?;

            sqlx::query("DELETE FROM retrieval_units WHERE unit_id = ?1")
                .bind(&unit_id)
                .execute(&mut *tx)
                .await
                .context("delete retrieval unit")?;

            result.rows_affected() > 0
        }
        ForgetMode::Tombstone => {
            sqlx::query(
                "INSERT INTO belief_slots (
                    entity_id, slot_key, value, status, winner_event_id, source,
                    confidence, importance, privacy_level, updated_at
                ) VALUES (?1, ?2, '', 'tombstoned', ?3, 'system', 1.0, 1.0, 'secret', ?4)
                ON CONFLICT(entity_id, slot_key) DO UPDATE SET
                    value = excluded.value,
                    status = excluded.status,
                    winner_event_id = excluded.winner_event_id,
                    source = excluded.source,
                    confidence = excluded.confidence,
                    importance = excluded.importance,
                    privacy_level = excluded.privacy_level,
                    updated_at = excluded.updated_at",
            )
            .bind(entity_id)
            .bind(slot_key)
            .bind(Uuid::new_v4().to_string())
            .bind(&now)
            .execute(&mut *tx)
            .await
            .context("tombstone belief slot")?;

            sqlx::query("DELETE FROM retrieval_units WHERE unit_id = ?1")
                .bind(&unit_id)
                .execute(&mut *tx)
                .await
                .context("delete retrieval unit for tombstone")?;

            true
        }
    };

    tx.commit().await.context("commit forget transaction")?;

    // Post-commit observations
    let slot_observed = observe_slot_artifact(pool, entity_id, slot_key).await;
    let retrieval_observed = observe_retrieval_artifact(pool, &unit_id).await;
    let cache_observed = ForgetArtifactObservation::Absent;
    let ledger_observed =
        observe_ledger_artifact(pool, entity_id, slot_key, phase, reason, &now).await?;

    let artifact_checks = vec![
        ForgetArtifactCheck::new(
            ForgetArtifact::Slot,
            mode.artifact_requirement(ForgetArtifact::Slot),
            slot_observed,
        ),
        ForgetArtifactCheck::new(
            ForgetArtifact::RetrievalUnits,
            mode.artifact_requirement(ForgetArtifact::RetrievalUnits),
            retrieval_observed,
        ),
        ForgetArtifactCheck::new(
            ForgetArtifact::Caches,
            mode.artifact_requirement(ForgetArtifact::Caches),
            cache_observed,
        ),
        ForgetArtifactCheck::new(
            ForgetArtifact::Ledger,
            mode.artifact_requirement(ForgetArtifact::Ledger),
            ledger_observed,
        ),
    ];

    Ok(ForgetOutcome::from_checks(
        entity_id,
        slot_key,
        mode,
        applied,
        false,
        artifact_checks,
    ))
}

async fn observe_slot_artifact(
    pool: &SqlitePool,
    entity_id: &str,
    slot_key: &str,
) -> ForgetArtifactObservation {
    let status: Option<(String,)> =
        sqlx::query_as("SELECT status FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2")
            .bind(entity_id)
            .bind(slot_key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    match status.as_ref().map(|s| s.0.as_str()) {
        None => ForgetArtifactObservation::Absent,
        Some("active") => ForgetArtifactObservation::PresentRetrievable,
        Some(_) => ForgetArtifactObservation::PresentNonRetrievable,
    }
}

async fn observe_retrieval_artifact(pool: &SqlitePool, unit_id: &str) -> ForgetArtifactObservation {
    let visibility: Option<(String,)> =
        sqlx::query_as("SELECT visibility FROM retrieval_units WHERE unit_id = ?1")
            .bind(unit_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();

    match visibility.as_ref().map(|v| v.0.as_str()) {
        None => ForgetArtifactObservation::Absent,
        Some("secret") => ForgetArtifactObservation::PresentNonRetrievable,
        Some(_) => ForgetArtifactObservation::PresentRetrievable,
    }
}

async fn observe_ledger_artifact(
    pool: &SqlitePool,
    entity_id: &str,
    slot_key: &str,
    phase: &str,
    reason: &str,
    executed_at: &str,
) -> anyhow::Result<ForgetArtifactObservation> {
    let exists: (i64,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1
            FROM deletion_ledger
            WHERE entity_id = ?1
              AND target_slot_key = ?2
              AND phase = ?3
              AND reason = ?4
              AND executed_at = ?5
        )",
    )
    .bind(entity_id)
    .bind(slot_key)
    .bind(phase)
    .bind(reason)
    .bind(executed_at)
    .fetch_one(pool)
    .await
    .context("check deletion ledger entry")?;

    Ok(if exists.0 == 1 {
        ForgetArtifactObservation::PresentNonRetrievable
    } else {
        ForgetArtifactObservation::Absent
    })
}

/// Count memory events, optionally filtered by `entity_id`.
pub(super) async fn count_events(
    pool: &SqlitePool,
    entity_id: Option<&str>,
) -> anyhow::Result<usize> {
    let count: (i64,) = if let Some(entity) = entity_id {
        sqlx::query_as("SELECT COUNT(*) FROM memory_events WHERE entity_id = ?1")
            .bind(entity)
            .fetch_one(pool)
            .await
            .context("count memory events by entity")?
    } else {
        sqlx::query_as("SELECT COUNT(*) FROM memory_events")
            .fetch_one(pool)
            .await
            .context("count all memory events")?
    };

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    Ok(count.0 as usize)
}
