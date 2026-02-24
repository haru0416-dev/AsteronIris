use super::{codec, events, projection, search};
use crate::memory::associations::MemoryAssociation;
use crate::memory::embeddings::EmbeddingProvider;
use crate::memory::types::{
    BeliefSlot, ForgetMode, ForgetOutcome, MemoryEvent, MemoryEventInput, MemoryRecallItem,
    MemorySource, RecallQuery, SignalTier,
};
use crate::memory::vector;
use anyhow::Context;
use chrono::Local;
use sqlx::SqlitePool;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

const TREND_TTL_DAYS: f64 = 30.0;
const TREND_DECAY_WINDOW_DAYS: f64 = 45.0;

const REVOKED_PROVENANCE_MARKERS: [&str; 2] = [
    "lancedb:degraded:soft_forget_marker_rewrite",
    "lancedb:degraded:tombstone_marker_rewrite",
];

/// Internal recall candidate used during multi-phase scoring.
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

/// Metadata row fetched for multi-phase scoring.
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

// ── Public repository operations ─────────────────────────────

/// Append a new memory event, project into belief slots / retrieval units.
pub(super) async fn append_event(
    pool: &SqlitePool,
    embedder: &Arc<dyn EmbeddingProvider>,
    cache_max: usize,
    input: MemoryEventInput,
) -> anyhow::Result<MemoryEvent> {
    let input = input.normalize_for_ingress()?;
    let embedding = get_or_compute_embedding(pool, embedder, cache_max, &input.value).await?;

    let meta = projection::prepare_event_metadata(&input, embedding);
    let (should_replace, supersedes_event_id) =
        projection::decide_replacement(pool, &input).await?;
    projection::insert_event_records(
        pool,
        &input,
        &meta,
        should_replace,
        supersedes_event_id.as_deref(),
    )
    .await?;

    Ok(projection::build_event_output(&input, &meta))
}

/// Recall memories via hybrid FTS5 + vector search with multi-phase scoring.
pub(super) async fn recall_scoped(
    pool: &SqlitePool,
    embedder: &Arc<dyn EmbeddingProvider>,
    cache_max: usize,
    query: RecallQuery,
) -> anyhow::Result<Vec<MemoryRecallItem>> {
    query.enforce_policy()?;
    if query.query.trim().is_empty() || query.limit == 0 {
        return Ok(Vec::new());
    }

    let search_limit = query.limit.saturating_mul(3);
    let query_embedding = get_or_compute_embedding(pool, embedder, cache_max, &query.query).await?;

    let fts_results =
        search::fts5_search_scoped(pool, &query.entity_id, &query.query, search_limit).await?;
    let vector_results = if let Some(ref embedding) = query_embedding {
        search::vector_search_scoped(pool, &query.entity_id, embedding, search_limit).await?
    } else {
        Vec::new()
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

    let candidate_ids: Vec<String> = merged.iter().map(|c| c.id.clone()).collect();
    multi_phase_score(pool, &query, &merged, &candidate_ids).await
}

/// Resolve a belief slot.
pub(super) async fn resolve_slot(
    pool: &SqlitePool,
    entity_id: &str,
    slot_key: &str,
) -> anyhow::Result<Option<BeliefSlot>> {
    events::resolve_slot(pool, entity_id, slot_key).await
}

/// Forget a belief slot.
pub(super) async fn forget_slot(
    pool: &SqlitePool,
    entity_id: &str,
    slot_key: &str,
    mode: ForgetMode,
    reason: &str,
) -> anyhow::Result<ForgetOutcome> {
    events::forget_slot(pool, entity_id, slot_key, mode, reason).await
}

/// Count memory events.
pub(super) async fn count_events(
    pool: &SqlitePool,
    entity_id: Option<&str>,
) -> anyhow::Result<usize> {
    events::count_events(pool, entity_id).await
}

/// Add an association between two memory entries.
pub(super) async fn add_association(
    pool: &SqlitePool,
    assoc: MemoryAssociation,
) -> anyhow::Result<()> {
    let kind_str = codec::association_kind_to_str(assoc.kind);
    sqlx::query(
        "INSERT INTO associations (source_id, target_id, kind, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(source_id, target_id, kind) DO UPDATE SET
             confidence = excluded.confidence,
             created_at = excluded.created_at",
    )
    .bind(&assoc.source_id)
    .bind(&assoc.target_id)
    .bind(kind_str)
    .bind(assoc.confidence)
    .bind(&assoc.created_at)
    .execute(pool)
    .await
    .context("insert association")?;
    Ok(())
}

/// Get all associations for a given entry (as source or target).
pub(super) async fn get_associations(
    pool: &SqlitePool,
    entry_id: &str,
) -> anyhow::Result<Vec<MemoryAssociation>> {
    let rows: Vec<(String, String, String, f64, String)> = sqlx::query_as(
        "SELECT source_id, target_id, kind, confidence, created_at
         FROM associations
         WHERE source_id = ?1 OR target_id = ?1",
    )
    .bind(entry_id)
    .fetch_all(pool)
    .await
    .context("query associations")?;

    let results = rows
        .into_iter()
        .map(
            |(source_id, target_id, kind, confidence, created_at)| MemoryAssociation {
                source_id,
                target_id,
                kind: codec::str_to_association_kind(&kind),
                confidence,
                created_at,
            },
        )
        .collect();
    Ok(results)
}

// ── Embedding cache ──────────────────────────────────────────

/// Deterministic content hash for embedding cache.
fn content_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(text.as_bytes());
    format!(
        "{:016x}",
        u64::from_be_bytes(hash[..8].try_into().unwrap_or([0u8; 8]))
    )
}

/// Get embedding from cache or compute + cache it.
async fn get_or_compute_embedding(
    pool: &SqlitePool,
    embedder: &Arc<dyn EmbeddingProvider>,
    cache_max: usize,
    text: &str,
) -> anyhow::Result<Option<Vec<f32>>> {
    if embedder.dimensions() == 0 {
        return Ok(None);
    }

    let hash = content_hash(text);
    let now = Local::now().to_rfc3339();

    // Check cache
    let cached: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT embedding FROM embedding_cache WHERE content_hash = ?1")
            .bind(&hash)
            .fetch_optional(pool)
            .await
            .context("embedding cache lookup")?;

    if let Some((bytes,)) = cached {
        sqlx::query("UPDATE embedding_cache SET accessed_at = ?1 WHERE content_hash = ?2")
            .bind(&now)
            .bind(&hash)
            .execute(pool)
            .await
            .context("update embedding cache access time")?;
        return Ok(Some(vector::bytes_to_vec(&bytes)));
    }

    // Compute embedding
    let embedding = embedder.embed_one(text).await?;
    let bytes = vector::vec_to_bytes(&embedding);

    // Store + LRU eviction
    sqlx::query(
        "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, created_at, accessed_at)
         VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&hash)
    .bind(&bytes)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .context("insert embedding into cache")?;

    #[allow(clippy::cast_possible_wrap)]
    let max = cache_max as i64;
    sqlx::query(
        "DELETE FROM embedding_cache WHERE content_hash IN (
            SELECT content_hash FROM embedding_cache
            ORDER BY accessed_at ASC
            LIMIT MAX(0, (SELECT COUNT(*) FROM embedding_cache) - ?1)
        )",
    )
    .bind(max)
    .execute(pool)
    .await
    .context("evict excess embedding cache entries")?;

    Ok(Some(embedding))
}

// ── Multi-phase scoring ──────────────────────────────────────

#[allow(clippy::too_many_lines)]
async fn multi_phase_score(
    pool: &SqlitePool,
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

    // Build the query with dynamic binds
    let mut db_query = sqlx::query_as::<
        _,
        (
            String,         // unit_id
            String,         // entity_id
            String,         // slot_key
            String,         // content
            f64,            // reliability
            f64,            // importance
            String,         // visibility
            String,         // updated_at
            f64,            // recency_score
            f64,            // contradiction_penalty
            String,         // signal_tier
            Option<String>, // provenance_source_class
            Option<String>, // provenance_reference
            Option<String>, // slot status
            i64,            // denylisted_by_ledger
        ),
    >(&sql);

    for id in candidate_ids {
        db_query = db_query.bind(id);
    }

    let rows = db_query
        .fetch_all(pool)
        .await
        .context("multi-phase metadata query")?;

    let mut metadata_by_id = HashMap::with_capacity(candidate_ids.len());
    for row in rows {
        let unit_id = row.0;
        let metadata = RecallMetadata {
            entity_id: row.1,
            slot_key: row.2,
            content: row.3,
            reliability: row.4,
            importance: row.5,
            visibility: row.6,
            updated_at: row.7,
            recency_score: row.8,
            contradiction_penalty: row.9,
            signal_tier: codec::str_to_signal_tier(&row.10),
            provenance_source_class: row.11,
            provenance_reference: row.12,
            slot_status: row.13,
            denylisted_by_ledger: row.14 != 0,
        };
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
                privacy_level: codec::str_to_privacy(&metadata.visibility),
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

        let days_since_update = days_since_now(&metadata.updated_at);
        let recency_decay =
            recency_decay_for_slot(&metadata.slot_key, days_since_update) * metadata.recency_score;
        let contradiction_penalty = metadata.contradiction_penalty.clamp(0.0, 1.0);
        let reliability = metadata.reliability.clamp(0.0, 1.0);
        let importance = metadata.importance.clamp(0.0, 1.0);

        let trend_boost = if metadata.signal_tier == SignalTier::Raw
            && is_trend_slot(&metadata.slot_key)
            && days_since_update <= TREND_TTL_DAYS
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

// ── Helpers ──────────────────────────────────────────────────

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
    if is_trend_slot(slot_key) {
        if days_since_update <= TREND_TTL_DAYS {
            1.0
        } else {
            (1.0 - ((days_since_update - TREND_TTL_DAYS) / TREND_DECAY_WINDOW_DAYS)).max(0.0)
        }
    } else {
        (1.0 - (days_since_update / 90.0)).max(0.20)
    }
}

/// Health check: execute a trivial query.
pub(super) async fn health_check(pool: &SqlitePool) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok()
}
