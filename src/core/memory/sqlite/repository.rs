use super::{MutexLockAnyhow, SqliteMemory};
use crate::core::memory::vector;
use crate::core::memory::{
    MemoryEvent, MemoryEventInput, MemoryEventType, MemoryRecallItem, MemorySource, RecallQuery,
    SignalTier,
};
use anyhow::Context;
use chrono::Local;
use rusqlite::params;
use std::cmp::Ordering;
use std::collections::HashMap;
use uuid::Uuid;

const REVOKED_PROVENANCE_MARKERS: [&str; 2] = [
    "lancedb:degraded:soft_forget_marker_rewrite",
    "lancedb:degraded:tombstone_marker_rewrite",
];

struct RecallCandidate {
    item: MemoryRecallItem,
    provenance_source_class: Option<String>,
    provenance_reference: Option<String>,
    slot_status: Option<String>,
    denylisted_by_ledger: bool,
}

struct RecallMetadata {
    entity_id: String,
    slot_key: String,
    content: String,
    reliability: f64,
    importance: f64,
    visibility: String,
    updated_at: String,
    recency_score: f64,
    contradiction_penalty: f64,
    signal_tier: SignalTier,
    provenance_source_class: Option<String>,
    provenance_reference: Option<String>,
    slot_status: Option<String>,
    denylisted_by_ledger: bool,
}

impl RecallCandidate {
    fn allowed_for_replay(&self) -> bool {
        if self.denylisted_by_ledger {
            return false;
        }

        if self.slot_status.as_deref() != Some("active") {
            return false;
        }

        let source_class = self.provenance_source_class.as_deref();
        let provenance_reference = self.provenance_reference.as_deref();
        if source_class == Some("system")
            && provenance_reference.is_some_and(|reference| {
                REVOKED_PROVENANCE_MARKERS
                    .iter()
                    .any(|marker| reference.eq_ignore_ascii_case(marker))
            })
        {
            return false;
        }

        true
    }
}

impl SqliteMemory {
    #[allow(clippy::too_many_lines, clippy::unused_async)]
    pub(super) async fn append_event(
        &self,
        input: MemoryEventInput,
    ) -> anyhow::Result<MemoryEvent> {
        let input = input.normalize_for_ingress()?;
        let embedding = self.get_or_compute_embedding(&input.value).await?;
        let conn = self.conn.lock_anyhow()?;

        let event_id = Uuid::new_v4().to_string();
        let ingested_at = Local::now().to_rfc3339();
        let source = Self::source_to_str(input.source);
        let layer = Self::layer_to_str(input.layer);
        let privacy = Self::privacy_to_str(&input.privacy_level);
        let event_type = input.event_type.to_string();
        let signal_tier = match input.event_type {
            MemoryEventType::InferredClaim => SignalTier::Inferred,
            MemoryEventType::ContradictionMarked => SignalTier::Governance,
            _ => input.signal_tier.unwrap_or(SignalTier::Belief),
        };
        let signal_tier_str = Self::signal_tier_to_str(signal_tier);
        let source_kind = input.source_kind.map(Self::source_kind_to_str);
        let source_uri = input.source_ref.clone();
        let provenance_source_class = input
            .provenance
            .as_ref()
            .map(|entry| Self::source_to_str(entry.source_class));
        let provenance_reference = input
            .provenance
            .as_ref()
            .map(|entry| entry.reference.clone());
        let provenance_evidence_uri = input
            .provenance
            .as_ref()
            .and_then(|entry| entry.evidence_uri.clone());
        let retention_tier = Self::retention_tier_for_layer(input.layer);
        let retention_expires_at =
            Self::retention_expiry_for_layer(input.layer, &input.occurred_at);
        let content_type = match input.event_type {
            MemoryEventType::FactAdded
            | MemoryEventType::FactUpdated
            | MemoryEventType::PreferenceSet
            | MemoryEventType::PreferenceUnset
            | MemoryEventType::SoftDeleted
            | MemoryEventType::HardDeleted
            | MemoryEventType::TombstoneWritten => "belief",
            MemoryEventType::InferredClaim => "inference",
            MemoryEventType::ContradictionMarked => "contradiction",
            MemoryEventType::SummaryCompacted => "summary",
        };
        let contradiction_penalty =
            if matches!(input.event_type, MemoryEventType::ContradictionMarked) {
                Self::contradiction_penalty(input.confidence, input.importance)
            } else {
                0.0
            };
        let promotion_status = if matches!(signal_tier, SignalTier::Raw) {
            "raw"
        } else {
            "promoted"
        };

        #[allow(clippy::cast_possible_wrap)]
        let embedding_dim = embedding.as_ref().map(|entry| entry.len() as i64);
        let embedding_blob = embedding.map(|entry| vector::vec_to_bytes(&entry));

        let mut incumbent_stmt = conn.prepare_cached(
            "SELECT winner_event_id, source, confidence, updated_at FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
        ).context("prepare belief slot lookup")?;
        let current: Option<(String, String, f64, String)> = incumbent_stmt
            .query_row(params![input.entity_id, input.slot_key], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .ok();

        let should_replace =
            if let Some((_, current_source, current_confidence, current_updated_at)) = &current {
                let current_priority = Self::source_priority(&Self::str_to_source(current_source));
                let incoming_priority = Self::source_priority(&input.source);
                match incoming_priority.cmp(&current_priority) {
                    Ordering::Greater => true,
                    Ordering::Less => false,
                    Ordering::Equal => match input.confidence.total_cmp(current_confidence) {
                        Ordering::Greater => true,
                        Ordering::Less => false,
                        Ordering::Equal => {
                            matches!(
                                Self::compare_normalized_timestamps(
                                    &input.occurred_at,
                                    current_updated_at
                                ),
                                Ordering::Greater
                            )
                        }
                    },
                }
            } else {
                true
            };

        let supersedes_event_id = current.as_ref().and_then(|(winner_event_id, _, _, _)| {
            if should_replace || matches!(input.event_type, MemoryEventType::ContradictionMarked) {
                Some(winner_event_id.clone())
            } else {
                None
            }
        });

        conn.execute(
            "INSERT INTO memory_events (
                event_id, entity_id, slot_key, layer, event_type, value, source,
                confidence, importance, provenance_source_class, provenance_reference,
                provenance_evidence_uri, retention_tier, retention_expires_at,
                signal_tier, source_kind,
                privacy_level, occurred_at, ingested_at, supersedes_event_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                event_id,
                input.entity_id,
                input.slot_key,
                layer,
                event_type,
                input.value,
                source,
                input.confidence,
                input.importance,
                provenance_source_class,
                provenance_reference,
                provenance_evidence_uri,
                retention_tier,
                retention_expires_at,
                signal_tier_str,
                source_kind,
                privacy,
                input.occurred_at,
                ingested_at,
                supersedes_event_id,
            ],
        )
        .context("insert memory event")?;

        if contradiction_penalty > 0.0 {
            let unit_id = format!("{}:{}", input.entity_id, input.slot_key);
            conn.execute(
                "UPDATE retrieval_units
                 SET contradiction_penalty = MIN(1.0, contradiction_penalty + ?2)
                 WHERE unit_id = ?1",
                params![unit_id, contradiction_penalty],
            )
            .context("update contradiction penalty")?;
        }

        if should_replace {
            conn.execute(
                "INSERT INTO belief_slots (
                    entity_id, slot_key, value, status, winner_event_id,
                    source, confidence, importance, privacy_level, updated_at
                ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(entity_id, slot_key) DO UPDATE SET
                    value = excluded.value,
                    status = excluded.status,
                    winner_event_id = excluded.winner_event_id,
                    source = excluded.source,
                    confidence = excluded.confidence,
                    importance = excluded.importance,
                    privacy_level = excluded.privacy_level,
                    updated_at = excluded.updated_at",
                params![
                    input.entity_id,
                    input.slot_key,
                    input.value,
                    event_id,
                    source,
                    input.confidence,
                    input.importance,
                    privacy,
                    input.occurred_at,
                ],
            )
            .context("upsert belief slot")?;

            let unit_id = format!("{}:{}", input.entity_id, input.slot_key);
            conn.execute(
                "INSERT INTO retrieval_units (
                    unit_id, entity_id, slot_key, content, content_type, signal_tier,
                    promotion_status, chunk_index, source_uri, source_kind,
                    recency_score, importance, reliability, contradiction_penalty, visibility,
                    embedding, embedding_model, embedding_dim,
                    layer, provenance_source_class, provenance_reference, provenance_evidence_uri,
                    retention_tier, retention_expires_at,
                    created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, 1.0, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?22)
                ON CONFLICT(unit_id) DO UPDATE SET
                    content = excluded.content,
                    content_type = excluded.content_type,
                    signal_tier = excluded.signal_tier,
                    promotion_status = excluded.promotion_status,
                    source_uri = excluded.source_uri,
                    source_kind = excluded.source_kind,
                    recency_score = excluded.recency_score,
                    importance = excluded.importance,
                    reliability = excluded.reliability,
                    contradiction_penalty = excluded.contradiction_penalty,
                    visibility = excluded.visibility,
                    embedding = excluded.embedding,
                    embedding_model = excluded.embedding_model,
                    embedding_dim = excluded.embedding_dim,
                    layer = excluded.layer,
                    provenance_source_class = excluded.provenance_source_class,
                    provenance_reference = excluded.provenance_reference,
                    provenance_evidence_uri = excluded.provenance_evidence_uri,
                    retention_tier = excluded.retention_tier,
                    retention_expires_at = excluded.retention_expires_at,
                    updated_at = excluded.updated_at",
                params![
                    unit_id,
                    input.entity_id,
                    input.slot_key,
                    input.value,
                    content_type,
                    signal_tier_str,
                    promotion_status,
                    source_uri,
                    source_kind,
                    input.importance,
                    input.confidence,
                    contradiction_penalty,
                    privacy,
                    embedding_blob,
                    embedding_dim,
                    layer,
                    provenance_source_class,
                    provenance_reference,
                    provenance_evidence_uri,
                    retention_tier,
                    retention_expires_at,
                    input.occurred_at,
                ],
            )
            .context("upsert retrieval unit")?;

            Self::try_promote_raw_to_candidate(&conn, &input.entity_id, &input.slot_key)
                .context("promote corroborated raw signal")?;
        }

        Ok(MemoryEvent {
            event_id,
            entity_id: input.entity_id,
            slot_key: input.slot_key,
            event_type: input.event_type,
            value: input.value,
            source: input.source,
            confidence: input.confidence,
            importance: input.importance,
            provenance: input.provenance,
            privacy_level: input.privacy_level,
            occurred_at: input.occurred_at,
            ingested_at,
        })
    }

    #[allow(clippy::unused_async, clippy::too_many_lines)]
    pub(super) async fn recall_scoped(
        &self,
        query: RecallQuery,
    ) -> anyhow::Result<Vec<MemoryRecallItem>> {
        query.enforce_policy()?;
        if query.query.trim().is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }

        let search_limit = query.limit.saturating_mul(3);
        let query_embedding = self.get_or_compute_embedding(&query.query).await?;

        let (fts_results, vector_results) = {
            let conn = self.conn.lock_anyhow()?;
            let fts =
                Self::fts5_search_scoped(&conn, &query.entity_id, &query.query, search_limit)?;
            let vec = if let Some(ref embedding) = query_embedding {
                Self::vector_search_scoped(&conn, &query.entity_id, embedding, search_limit)?
            } else {
                Vec::new()
            };
            (fts, vec)
        };

        let merged = if vector_results.is_empty() && fts_results.is_empty() {
            return Ok(Vec::new());
        } else if vector_results.is_empty() {
            fts_results
                .iter()
                .map(|(id, score)| vector::ScoredResult {
                    id: id.clone(),
                    vector_score: None,
                    keyword_score: Some(*score),
                    final_score: *score,
                })
                .collect::<Vec<_>>()
        } else {
            vector::rrf_merge(&vector_results, &fts_results, search_limit)
        };

        let candidate_ids = merged
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        let conn = self.conn.lock_anyhow()?;
        Self::multi_phase_score(&conn, &query, &merged, &candidate_ids)
    }

    fn try_promote_raw_to_candidate(
        conn: &rusqlite::Connection,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<()> {
        let distinct_sources: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT source) FROM memory_events WHERE entity_id = ?1 AND slot_key = ?2",
            params![entity_id, slot_key],
            |row| row.get(0),
        )?;

        if distinct_sources >= 2 {
            let unit_id = format!("{entity_id}:{slot_key}");
            conn.execute(
                "UPDATE retrieval_units
                 SET promotion_status = 'candidate'
                 WHERE unit_id = ?1 AND promotion_status = 'raw'",
                params![unit_id],
            )?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn multi_phase_score(
        conn: &std::sync::MutexGuard<'_, rusqlite::Connection>,
        query: &RecallQuery,
        rrf_candidates: &[vector::ScoredResult],
        candidate_ids: &[String],
    ) -> anyhow::Result<Vec<MemoryRecallItem>> {
        if candidate_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = std::iter::repeat_n("?", candidate_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT ru.unit_id, ru.entity_id, ru.slot_key, ru.content,
                    ru.reliability, ru.importance, ru.visibility, ru.updated_at,
                    ru.recency_score, ru.contradiction_penalty, ru.signal_tier,
                    ru.provenance_source_class, ru.provenance_reference,
                    bs.status,
                    EXISTS(
                        SELECT 1 FROM deletion_ledger dl
                        WHERE dl.entity_id = ru.entity_id
                          AND dl.target_slot_key = ru.slot_key
                          AND dl.phase IN ('soft', 'hard', 'tombstone')
                    ) AS denylisted_by_ledger
             FROM retrieval_units ru
             LEFT JOIN belief_slots bs ON bs.entity_id = ru.entity_id AND bs.slot_key = ru.slot_key
             WHERE ru.unit_id IN ({placeholders})"
        );

        let mut stmt = conn
            .prepare(&sql)
            .context("prepare multi-phase metadata query")?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(candidate_ids.iter()), |row| {
                let signal_tier_raw: String = row.get(10)?;
                Ok((
                    row.get::<_, String>(0)?,
                    RecallMetadata {
                        entity_id: row.get(1)?,
                        slot_key: row.get(2)?,
                        content: row.get(3)?,
                        reliability: row.get(4)?,
                        importance: row.get(5)?,
                        visibility: row.get(6)?,
                        updated_at: row.get(7)?,
                        recency_score: row.get(8)?,
                        contradiction_penalty: row.get(9)?,
                        signal_tier: Self::str_to_signal_tier(&signal_tier_raw),
                        provenance_source_class: row.get(11)?,
                        provenance_reference: row.get(12)?,
                        slot_status: row.get(13)?,
                        denylisted_by_ledger: row.get::<_, i64>(14)? != 0,
                    },
                ))
            })
            .context("execute multi-phase metadata query")?;

        let mut metadata_by_id = HashMap::with_capacity(candidate_ids.len());
        for row in rows {
            let (unit_id, metadata) = row.context("read multi-phase metadata row")?;
            metadata_by_id.insert(unit_id, metadata);
        }

        let mut results = Vec::with_capacity(query.limit);
        for scored in rrf_candidates {
            let Some(metadata) = metadata_by_id.get(&scored.id) else {
                continue;
            };

            let base_candidate = RecallCandidate {
                item: MemoryRecallItem {
                    entity_id: metadata.entity_id.clone(),
                    slot_key: metadata.slot_key.clone(),
                    value: metadata.content.clone(),
                    source: MemorySource::System,
                    confidence: metadata.reliability.clamp(0.0, 1.0),
                    importance: metadata.importance.clamp(0.0, 1.0),
                    privacy_level: Self::str_to_privacy(&metadata.visibility),
                    score: 0.0,
                    occurred_at: metadata.updated_at.clone(),
                },
                provenance_source_class: metadata.provenance_source_class.clone(),
                provenance_reference: metadata.provenance_reference.clone(),
                slot_status: metadata.slot_status.clone(),
                denylisted_by_ledger: metadata.denylisted_by_ledger,
            };
            if !base_candidate.allowed_for_replay() {
                continue;
            }

            let days_since_update = Self::days_since_now(&metadata.updated_at);
            let recency_decay = Self::recency_decay_for_slot(&metadata.slot_key, days_since_update)
                * metadata.recency_score;
            let contradiction_penalty = metadata.contradiction_penalty.clamp(0.0, 1.0);
            let reliability = metadata.reliability.clamp(0.0, 1.0);
            let importance = metadata.importance.clamp(0.0, 1.0);

            let trend_boost = if metadata.signal_tier == SignalTier::Raw
                && Self::is_trend_slot(&metadata.slot_key)
                && days_since_update <= Self::TREND_TTL_DAYS
            {
                0.05
            } else {
                0.0
            };

            let phase_score =
                (f64::from(scored.final_score) + trend_boost - contradiction_penalty).max(0.0);
            let metadata_score = (0.40 * recency_decay + 0.30 * importance + 0.30 * reliability)
                * (1.0 - contradiction_penalty);
            let final_score = 0.80 * phase_score + 0.20 * metadata_score;

            results.push(MemoryRecallItem {
                score: final_score,
                ..base_candidate.item
            });
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        results.truncate(query.limit);
        Ok(results)
    }

    fn is_trend_slot(slot_key: &str) -> bool {
        slot_key.starts_with("trend.")
            || slot_key.starts_with("trend/")
            || slot_key.contains(".trend.")
            || slot_key.contains("/trend/")
    }

    fn days_since_now(updated_at: &str) -> f64 {
        chrono::DateTime::parse_from_rfc3339(updated_at)
            .map(|ts| {
                let now = chrono::Utc::now();
                let then = ts.with_timezone(&chrono::Utc);
                (now - then)
                    .to_std()
                    .map_or(0.0, |duration| duration.as_secs_f64() / 86_400.0)
            })
            .unwrap_or(0.0)
    }

    fn recency_decay_for_slot(slot_key: &str, days_since_update: f64) -> f64 {
        if Self::is_trend_slot(slot_key) {
            if days_since_update <= Self::TREND_TTL_DAYS {
                1.0
            } else {
                (1.0 - ((days_since_update - Self::TREND_TTL_DAYS) / Self::TREND_DECAY_WINDOW_DAYS))
                    .max(0.0)
            }
        } else {
            (1.0 - (days_since_update / 90.0)).max(0.20)
        }
    }
}
