use super::{MutexLockAnyhow, SqliteMemory};
use crate::core::memory::{
    BeliefSlot, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation, ForgetMode,
    ForgetOutcome,
};
use anyhow::Context;
use chrono::Local;
use rusqlite::params;
use uuid::Uuid;

#[allow(clippy::too_many_lines, clippy::unused_async)]
impl SqliteMemory {
    pub(super) async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        let conn = self.conn.lock_anyhow()?;

        let mut stmt = conn
            .prepare(
                "SELECT value, source, confidence, importance, privacy_level, updated_at
             FROM belief_slots
             WHERE entity_id = ?1 AND slot_key = ?2 AND status = 'active'",
            )
            .context("prepare belief slot query")?;

        let row = stmt
            .query_row(params![entity_id, slot_key], |row| {
                Ok(BeliefSlot {
                    entity_id: entity_id.to_string(),
                    slot_key: slot_key.to_string(),
                    value: row.get(0)?,
                    source: Self::str_to_source(&row.get::<_, String>(1)?),
                    confidence: row.get(2)?,
                    importance: row.get(3)?,
                    privacy_level: Self::str_to_privacy(&row.get::<_, String>(4)?),
                    updated_at: row.get(5)?,
                })
            })
            .ok();
        Ok(row)
    }

    pub(super) async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        let conn = self.conn.lock_anyhow()?;
        let now = Local::now().to_rfc3339();
        let phase = match mode {
            ForgetMode::Soft => "soft",
            ForgetMode::Hard => "hard",
            ForgetMode::Tombstone => "tombstone",
        };

        conn.execute(
            "INSERT INTO deletion_ledger (
                ledger_id, entity_id, target_slot_key, phase, reason, requested_by, executed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'memory_forget', ?6)",
            params![
                Uuid::new_v4().to_string(),
                entity_id,
                slot_key,
                phase,
                reason,
                now
            ],
        )
        .context("insert deletion ledger entry")?;

        let doc_id = format!("{entity_id}:{slot_key}");
        let projection_content: Option<String> = conn
            .query_row(
                "SELECT content FROM memories WHERE key = ?1",
                params![slot_key],
                |row| row.get(0),
            )
            .ok();
        let projection_cache_hash = projection_content.as_deref().map(Self::content_hash);
        let applied = match mode {
            ForgetMode::Soft => {
                let affected_slot = conn
                    .execute(
                        "UPDATE belief_slots SET status = 'soft_deleted', updated_at = ?3
                     WHERE entity_id = ?1 AND slot_key = ?2",
                        params![entity_id, slot_key, now],
                    )
                    .context("soft delete belief slot")?;
                let _ = conn.execute(
                    "UPDATE retrieval_docs SET visibility = 'secret', updated_at = ?2 WHERE doc_id = ?1",
                    params![doc_id, now],
                )?;
                affected_slot > 0
            }
            ForgetMode::Hard => {
                let affected_slot = conn
                    .execute(
                        "DELETE FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
                        params![entity_id, slot_key],
                    )
                    .context("delete belief slot")?;
                let _ = conn.execute(
                    "DELETE FROM retrieval_docs WHERE doc_id = ?1",
                    params![doc_id],
                )?;
                let _ = conn.execute("DELETE FROM memories WHERE key = ?1", params![slot_key])?;
                if let Some(cache_hash) = &projection_cache_hash {
                    let _ = conn.execute(
                        "DELETE FROM embedding_cache WHERE content_hash = ?1",
                        params![cache_hash],
                    )?;
                }
                affected_slot > 0
            }
            ForgetMode::Tombstone => {
                conn.execute(
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
                    params![entity_id, slot_key, Uuid::new_v4().to_string(), now],
                )
                .context("tombstone belief slot")?;
                let _ = conn.execute(
                    "DELETE FROM retrieval_docs WHERE doc_id = ?1",
                    params![doc_id],
                )?;
                let _ = conn.execute("DELETE FROM memories WHERE key = ?1", params![slot_key])?;
                if let Some(cache_hash) = &projection_cache_hash {
                    let _ = conn.execute(
                        "DELETE FROM embedding_cache WHERE content_hash = ?1",
                        params![cache_hash],
                    )?;
                }
                true
            }
        };

        let slot_observed = Self::observe_slot_artifact(&conn, entity_id, slot_key);
        let retrieval_observed = Self::observe_retrieval_artifact(&conn, &doc_id);
        let projection_observed = Self::observe_projection_artifact(&conn, slot_key)?;
        let cache_observed = Self::observe_cache_artifact(&conn, projection_cache_hash.as_deref())?;
        let ledger_observed =
            Self::observe_ledger_artifact(&conn, entity_id, slot_key, phase, reason, &now)?;

        let artifact_checks = vec![
            ForgetArtifactCheck::new(
                ForgetArtifact::Slot,
                mode.artifact_requirement(ForgetArtifact::Slot),
                slot_observed,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::RetrievalDocs,
                mode.artifact_requirement(ForgetArtifact::RetrievalDocs),
                retrieval_observed,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::ProjectionDocs,
                mode.artifact_requirement(ForgetArtifact::ProjectionDocs),
                projection_observed,
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

    fn observe_slot_artifact(
        conn: &rusqlite::Connection,
        entity_id: &str,
        slot_key: &str,
    ) -> ForgetArtifactObservation {
        let slot_status: Option<String> = conn
            .query_row(
                "SELECT status FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
                params![entity_id, slot_key],
                |row| row.get(0),
            )
            .ok();

        match slot_status.as_deref() {
            None => ForgetArtifactObservation::Absent,
            Some("active") => ForgetArtifactObservation::PresentRetrievable,
            Some(_) => ForgetArtifactObservation::PresentNonRetrievable,
        }
    }

    fn observe_retrieval_artifact(
        conn: &rusqlite::Connection,
        doc_id: &str,
    ) -> ForgetArtifactObservation {
        let visibility: Option<String> = conn
            .query_row(
                "SELECT visibility FROM retrieval_docs WHERE doc_id = ?1",
                params![doc_id],
                |row| row.get(0),
            )
            .ok();

        match visibility.as_deref() {
            None => ForgetArtifactObservation::Absent,
            Some("secret") => ForgetArtifactObservation::PresentNonRetrievable,
            Some(_) => ForgetArtifactObservation::PresentRetrievable,
        }
    }

    fn observe_projection_artifact(
        conn: &rusqlite::Connection,
        slot_key: &str,
    ) -> anyhow::Result<ForgetArtifactObservation> {
        let exists = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM memories WHERE key = ?1)",
                params![slot_key],
                |row| row.get::<_, i64>(0),
            )
            .context("check projection entry existence")?
            == 1;

        Ok(if exists {
            ForgetArtifactObservation::PresentRetrievable
        } else {
            ForgetArtifactObservation::Absent
        })
    }

    fn observe_cache_artifact(
        conn: &rusqlite::Connection,
        cache_hash: Option<&str>,
    ) -> anyhow::Result<ForgetArtifactObservation> {
        let Some(cache_hash) = cache_hash else {
            return Ok(ForgetArtifactObservation::Absent);
        };

        let exists = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM embedding_cache WHERE content_hash = ?1)",
                params![cache_hash],
                |row| row.get::<_, i64>(0),
            )
            .context("check embedding cache entry")?
            == 1;

        Ok(if exists {
            ForgetArtifactObservation::PresentRetrievable
        } else {
            ForgetArtifactObservation::Absent
        })
    }

    fn observe_ledger_artifact(
        conn: &rusqlite::Connection,
        entity_id: &str,
        slot_key: &str,
        phase: &str,
        reason: &str,
        executed_at: &str,
    ) -> anyhow::Result<ForgetArtifactObservation> {
        let exists = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM deletion_ledger
                    WHERE entity_id = ?1
                      AND target_slot_key = ?2
                      AND phase = ?3
                      AND reason = ?4
                      AND executed_at = ?5
                )",
                params![entity_id, slot_key, phase, reason, executed_at],
                |row| row.get::<_, i64>(0),
            )
            .context("check deletion ledger entry")?
            == 1;

        Ok(if exists {
            ForgetArtifactObservation::PresentNonRetrievable
        } else {
            ForgetArtifactObservation::Absent
        })
    }

    pub(super) async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        let conn = self.conn.lock_anyhow()?;

        let count: i64 = if let Some(entity) = entity_id {
            conn.query_row(
                "SELECT COUNT(*) FROM memory_events WHERE entity_id = ?1",
                params![entity],
                |row| row.get(0),
            )
            .context("count memory events by entity")?
        } else {
            conn.query_row("SELECT COUNT(*) FROM memory_events", [], |row| row.get(0))
                .context("count all memory events")?
        };

        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Ok(count as usize)
    }
}
