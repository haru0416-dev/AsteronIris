use super::SqliteMemory;
use crate::memory::traits::MemoryLayer;
use crate::memory::vector;
use crate::memory::{
    MemoryCategory, MemoryEntry, MemoryEvent, MemoryEventInput, MemoryEventType, MemoryRecallItem,
    MemorySource, RecallQuery,
};
use chrono::Local;
use rusqlite::{params, ToSql};
use std::cmp::Ordering;
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
    pub(super) async fn append_event(
        &self,
        input: MemoryEventInput,
    ) -> anyhow::Result<MemoryEvent> {
        let input = input.normalize_for_ingress()?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let event_id = Uuid::new_v4().to_string();
        let ingested_at = Local::now().to_rfc3339();
        let source = Self::source_to_str(&input.source);
        let layer = Self::layer_to_str(&input.layer);
        let privacy = Self::privacy_to_str(&input.privacy_level);
        let event_type = input.event_type.to_string();
        let provenance_source_class = input
            .provenance
            .as_ref()
            .map(|entry| Self::source_to_str(&entry.source_class));
        let provenance_reference = input
            .provenance
            .as_ref()
            .map(|entry| entry.reference.clone());
        let provenance_evidence_uri = input
            .provenance
            .as_ref()
            .and_then(|entry| entry.evidence_uri.clone());
        let retention_tier = Self::retention_tier_for_layer(&input.layer);
        let retention_expires_at =
            Self::retention_expiry_for_layer(&input.layer, &input.occurred_at);
        let contradiction_penalty =
            if matches!(input.event_type, MemoryEventType::ContradictionMarked) {
                Self::contradiction_penalty(input.confidence, input.importance)
            } else {
                0.0
            };

        let mut incumbent_stmt = conn.prepare(
            "SELECT winner_event_id, source, confidence, updated_at FROM belief_slots WHERE entity_id = ?1 AND slot_key = ?2",
        )?;
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
                privacy_level, occurred_at, ingested_at, supersedes_event_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
                privacy,
                input.occurred_at,
                ingested_at,
                supersedes_event_id,
            ],
        )?;

        if contradiction_penalty > 0.0 {
            let doc_id = format!("{}:{}", input.entity_id, input.slot_key);
            conn.execute(
                "UPDATE retrieval_docs
                 SET contradiction_penalty = MIN(1.0, contradiction_penalty + ?2)
                 WHERE doc_id = ?1",
                params![doc_id, contradiction_penalty],
            )?;
        }

        let shadow_id = Uuid::new_v4().to_string();
        let shadow_category = if input.slot_key.starts_with("persona/") {
            "persona"
        } else {
            match input.source {
                MemorySource::ExplicitUser | MemorySource::ToolVerified => "core",
                MemorySource::System => "daily",
                MemorySource::Inferred => "conversation",
            }
        };

        conn.execute(
            "INSERT INTO memories (
                id, key, content, category, layer,
                provenance_source_class, provenance_reference, provenance_evidence_uri,
                retention_tier, retention_expires_at,
                embedding, created_at, updated_at
            )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?11)
             ON CONFLICT(key) DO UPDATE SET
                content = excluded.content,
                category = excluded.category,
                layer = excluded.layer,
                provenance_source_class = excluded.provenance_source_class,
                provenance_reference = excluded.provenance_reference,
                provenance_evidence_uri = excluded.provenance_evidence_uri,
                retention_tier = excluded.retention_tier,
                retention_expires_at = excluded.retention_expires_at,
                updated_at = excluded.updated_at",
            params![
                shadow_id,
                input.slot_key,
                input.value,
                shadow_category,
                layer,
                provenance_source_class,
                provenance_reference,
                provenance_evidence_uri,
                retention_tier,
                retention_expires_at,
                input.occurred_at,
            ],
        )?;

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
            )?;

            let doc_id = format!("{}:{}", input.entity_id, input.slot_key);
            conn.execute(
                "INSERT INTO retrieval_docs (
                    doc_id, entity_id, slot_key, text_body, layer,
                    provenance_source_class, provenance_reference, provenance_evidence_uri,
                    retention_tier, retention_expires_at,
                    recency_score, importance, reliability, contradiction_penalty, visibility, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1.0, ?11, ?12, ?13, ?14, ?15)
                ON CONFLICT(doc_id) DO UPDATE SET
                    text_body = excluded.text_body,
                    layer = excluded.layer,
                    provenance_source_class = excluded.provenance_source_class,
                    provenance_reference = excluded.provenance_reference,
                    provenance_evidence_uri = excluded.provenance_evidence_uri,
                    retention_tier = excluded.retention_tier,
                    retention_expires_at = excluded.retention_expires_at,
                    recency_score = excluded.recency_score,
                    importance = excluded.importance,
                    reliability = excluded.reliability,
                    contradiction_penalty = excluded.contradiction_penalty,
                    visibility = excluded.visibility,
                    updated_at = excluded.updated_at",
                params![
                    doc_id,
                    input.entity_id,
                    input.slot_key,
                    input.value,
                    layer,
                    provenance_source_class,
                    provenance_reference,
                    provenance_evidence_uri,
                    retention_tier,
                    retention_expires_at,
                    input.importance,
                    input.confidence,
                    contradiction_penalty,
                    privacy,
                    input.occurred_at,
                ],
            )?;
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

    pub(super) async fn recall_scoped(
        &self,
        query: RecallQuery,
    ) -> anyhow::Result<Vec<MemoryRecallItem>> {
        query.enforce_policy()?;

        if query.query.trim().is_empty() || query.limit == 0 {
            return Ok(Vec::new());
        }

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let like_query = format!("%{}%", query.query);
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = query.limit as i64;
        let mut stmt = conn.prepare(
            "SELECT rd.entity_id, rd.slot_key, rd.text_body, rd.reliability, rd.importance, rd.visibility, rd.updated_at,
                    rd.provenance_source_class, rd.provenance_reference, bs.status,
                    EXISTS(
                        SELECT 1
                        FROM deletion_ledger dl
                        WHERE dl.entity_id = rd.entity_id
                          AND dl.target_slot_key = rd.slot_key
                          AND dl.phase IN ('soft', 'hard', 'tombstone')
                    ) AS denylisted_by_ledger,
                    (
                        0.45 * ((0.60 * rd.importance) + (0.40 * rd.reliability))
                      + 0.35 * (
                            rd.recency_score *
                            CASE
                                WHEN rd.slot_key LIKE 'trend.%'
                                  OR rd.slot_key LIKE 'trend/%'
                                  OR rd.slot_key LIKE '%.trend.%'
                                  OR rd.slot_key LIKE '%/trend/%'
                                THEN
                                    CASE
                                        WHEN COALESCE(julianday('now') - julianday(rd.updated_at), 0.0) <= ?3
                                        THEN 1.0
                                        ELSE MAX(
                                            0.0,
                                            1.0 - (
                                                (COALESCE(julianday('now') - julianday(rd.updated_at), 0.0) - ?3) / ?4
                                            )
                                        )
                                    END
                                ELSE
                                    MAX(
                                        0.20,
                                        1.0 - (COALESCE(julianday('now') - julianday(rd.updated_at), 0.0) / 90.0)
                                    )
                            END
                        )
                      + 0.20 * CASE WHEN rd.text_body LIKE ?2 THEN 1.0 ELSE 0.0 END
                      - rd.contradiction_penalty
                    ) AS final_score
             FROM retrieval_docs rd
             LEFT JOIN belief_slots bs
               ON bs.entity_id = rd.entity_id
              AND bs.slot_key = rd.slot_key
             WHERE rd.entity_id = ?1
                AND rd.visibility != 'secret'
                AND rd.text_body LIKE ?2
              ORDER BY final_score DESC, rd.updated_at DESC, rd.doc_id ASC
              LIMIT ?5",
        )?;

        let rows = stmt.query_map(
            params![
                query.entity_id,
                like_query,
                Self::TREND_TTL_DAYS,
                Self::TREND_DECAY_WINDOW_DAYS,
                limit_i64
            ],
            |row| {
                let visibility: String = row.get(5)?;
                Ok(RecallCandidate {
                    item: MemoryRecallItem {
                        entity_id: row.get(0)?,
                        slot_key: row.get(1)?,
                        value: row.get(2)?,
                        source: MemorySource::System,
                        confidence: row.get(3)?,
                        importance: row.get(4)?,
                        privacy_level: Self::str_to_privacy(&visibility),
                        score: row.get(11)?,
                        occurred_at: row.get(6)?,
                    },
                    provenance_source_class: row.get(7)?,
                    provenance_reference: row.get(8)?,
                    slot_status: row.get(9)?,
                    denylisted_by_ledger: row.get::<_, i64>(10)? != 0,
                })
            },
        )?;

        let mut out = Vec::new();
        for row in rows {
            let candidate = row?;
            if candidate.allowed_for_replay() {
                out.push(candidate.item);
            }
        }
        Ok(out)
    }

    pub(super) async fn upsert_projection_entry(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
    ) -> anyhow::Result<()> {
        let embedding_bytes = self
            .get_or_compute_embedding(content)
            .await?
            .map(|emb| vector::vec_to_bytes(&emb));

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let now = Local::now().to_rfc3339();
        let cat = Self::category_to_str(&category);
        let layer = Self::layer_to_str(&MemoryLayer::Working);
        let retention_tier = Self::retention_tier_for_layer(&MemoryLayer::Working);
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO memories (
                id, key, content, category, layer,
                provenance_source_class, provenance_reference, provenance_evidence_uri,
                retention_tier, retention_expires_at,
                embedding, created_at, updated_at
            )
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, NULL, ?7, ?8, ?9)
             ON CONFLICT(key) DO UPDATE SET
                content = excluded.content,
                category = excluded.category,
                layer = excluded.layer,
                provenance_source_class = excluded.provenance_source_class,
                provenance_reference = excluded.provenance_reference,
                provenance_evidence_uri = excluded.provenance_evidence_uri,
                retention_tier = excluded.retention_tier,
                retention_expires_at = excluded.retention_expires_at,
                embedding = excluded.embedding,
                updated_at = excluded.updated_at",
            params![
                id,
                key,
                content,
                cat,
                layer,
                retention_tier,
                embedding_bytes,
                now,
                now
            ],
        )?;

        Ok(())
    }

    pub(super) async fn search_projection(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let query_embedding = self.get_or_compute_embedding(query).await?;

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let search_limit = limit.saturating_mul(2);
        let keyword_results = Self::fts5_search(&conn, query, search_limit).unwrap_or_default();

        let vector_results = if let Some(ref qe) = query_embedding {
            Self::vector_search(&conn, qe, search_limit).unwrap_or_default()
        } else {
            Vec::new()
        };

        let merged = if vector_results.is_empty() {
            keyword_results
                .iter()
                .map(|(id, score)| vector::ScoredResult {
                    id: id.clone(),
                    vector_score: None,
                    keyword_score: Some(*score),
                    final_score: *score,
                })
                .collect::<Vec<_>>()
        } else {
            vector::hybrid_merge(
                &vector_results,
                &keyword_results,
                self.vector_weight,
                self.keyword_weight,
                limit,
            )
        };

        let mut results = Vec::new();
        let mut by_id_stmt = conn
            .prepare("SELECT id, key, content, category, created_at FROM memories WHERE id = ?1")?;
        for scored in &merged {
            if let Ok(entry) = by_id_stmt.query_row(params![scored.id], |row| {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    key: row.get(1)?,
                    content: row.get(2)?,
                    category: Self::str_to_category(&row.get::<_, String>(3)?),
                    timestamp: row.get(4)?,
                    session_id: None,
                    score: Some(f64::from(scored.final_score)),
                })
            }) {
                results.push(entry);
            }
        }

        if results.is_empty() {
            let keywords: Vec<String> =
                query.split_whitespace().map(|w| format!("%{w}%")).collect();
            if !keywords.is_empty() {
                let conditions: Vec<String> = keywords
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        format!("(content LIKE ?{} OR key LIKE ?{})", i * 2 + 1, i * 2 + 2)
                    })
                    .collect();
                let where_clause = conditions.join(" OR ");
                let sql = format!(
                    "SELECT id, key, content, category, created_at FROM memories
                     WHERE {where_clause}
                     ORDER BY updated_at DESC
                     LIMIT ?{}",
                    keywords.len() * 2 + 1
                );
                let mut stmt = conn.prepare(&sql)?;
                let mut param_values: Vec<&dyn ToSql> = Vec::with_capacity(keywords.len() * 2 + 1);
                for kw in &keywords {
                    param_values.push(kw);
                    param_values.push(kw);
                }
                #[allow(clippy::cast_possible_wrap)]
                let limit_i64 = limit as i64;
                param_values.push(&limit_i64);
                let rows = stmt.query_map(param_values.as_slice(), |row| {
                    Ok(MemoryEntry {
                        id: row.get(0)?,
                        key: row.get(1)?,
                        content: row.get(2)?,
                        category: Self::str_to_category(&row.get::<_, String>(3)?),
                        timestamp: row.get(4)?,
                        session_id: None,
                        score: Some(1.0),
                    })
                })?;
                for row in rows {
                    results.push(row?);
                }
            }
        }

        results.truncate(limit);
        Ok(results)
    }

    pub(super) async fn fetch_projection_entry(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<MemoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut stmt = conn.prepare(
            "SELECT id, key, content, category, created_at FROM memories WHERE key = ?1",
        )?;

        let mut rows = stmt.query_map(params![key], |row| {
            Ok(MemoryEntry {
                id: row.get(0)?,
                key: row.get(1)?,
                content: row.get(2)?,
                category: Self::str_to_category(&row.get::<_, String>(3)?),
                timestamp: row.get(4)?,
                session_id: None,
                score: None,
            })
        })?;

        match rows.next() {
            Some(Ok(entry)) => Ok(Some(entry)),
            _ => Ok(None),
        }
    }

    pub(super) async fn list_projection_entries(
        &self,
        category: Option<&MemoryCategory>,
    ) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

        let mut results = Vec::new();

        let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<MemoryEntry> {
            Ok(MemoryEntry {
                id: row.get(0)?,
                key: row.get(1)?,
                content: row.get(2)?,
                category: Self::str_to_category(&row.get::<_, String>(3)?),
                timestamp: row.get(4)?,
                session_id: None,
                score: None,
            })
        };

        if let Some(cat) = category {
            let cat_str = Self::category_to_str(cat);
            let mut stmt = conn.prepare(
                "SELECT id, key, content, category, created_at FROM memories
                 WHERE category = ?1 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map(params![cat_str], row_mapper)?;
            for row in rows {
                results.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, key, content, category, created_at FROM memories
                 ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], row_mapper)?;
            for row in rows {
                results.push(row?);
            }
        }

        Ok(results)
    }

    pub(super) async fn delete_projection_entry(&self, key: &str) -> anyhow::Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let affected = conn.execute("DELETE FROM memories WHERE key = ?1", params![key])?;
        Ok(affected > 0)
    }

    pub(super) async fn count_projection_entries(&self) -> anyhow::Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        Ok(count as usize)
    }
}
