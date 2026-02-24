use super::codec;
use crate::memory::types::{MemoryEvent, MemoryEventInput, MemoryEventType, SignalTier};
use crate::memory::vector;
use anyhow::Context;
use chrono::Local;
use sqlx::SqlitePool;
use std::cmp::Ordering;
use uuid::Uuid;

/// Prepared metadata for an event insertion.
pub(super) struct EventMetadata {
    pub event_id: String,
    pub ingested_at: String,
    pub source: &'static str,
    pub layer: &'static str,
    pub privacy: &'static str,
    pub event_type: String,
    pub signal_tier_str: &'static str,
    pub source_kind: Option<&'static str>,
    pub source_uri: Option<String>,
    pub provenance_source_class: Option<&'static str>,
    pub provenance_reference: Option<String>,
    pub provenance_evidence_uri: Option<String>,
    pub retention_tier: &'static str,
    pub retention_expires_at: Option<String>,
    pub content_type: &'static str,
    pub contradiction_penalty: f64,
    pub promotion_status: &'static str,
    pub embedding_dim: Option<i64>,
    pub embedding_blob: Option<Vec<u8>>,
}

/// Derive all metadata fields from the input and optional embedding.
pub(super) fn prepare_event_metadata(
    input: &MemoryEventInput,
    embedding: Option<Vec<f32>>,
) -> EventMetadata {
    let event_id = Uuid::new_v4().to_string();
    let ingested_at = Local::now().to_rfc3339();
    let source = codec::source_to_str(input.source);
    let layer = codec::layer_to_str(input.layer);
    let privacy = codec::privacy_to_str(&input.privacy_level);
    let event_type = input.event_type.to_string();

    let signal_tier = match input.event_type {
        MemoryEventType::InferredClaim => SignalTier::Inferred,
        MemoryEventType::ContradictionMarked => SignalTier::Governance,
        _ => input.signal_tier.unwrap_or(SignalTier::Belief),
    };
    let signal_tier_str = codec::signal_tier_to_str(signal_tier);

    let source_kind = input.source_kind.map(codec::source_kind_to_str);
    let source_uri = input.source_ref.clone();

    let provenance_source_class = input
        .provenance
        .as_ref()
        .map(|p| codec::source_to_str(p.source_class));
    let provenance_reference = input.provenance.as_ref().map(|p| p.reference.clone());
    let provenance_evidence_uri = input
        .provenance
        .as_ref()
        .and_then(|p| p.evidence_uri.clone());

    let retention_tier = codec::retention_tier_for_layer(input.layer);
    let retention_expires_at = codec::retention_expiry_for_layer(input.layer, &input.occurred_at);

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

    let contradiction_penalty = if matches!(input.event_type, MemoryEventType::ContradictionMarked)
    {
        codec::contradiction_penalty(input.confidence, input.importance)
    } else {
        0.0
    };

    let promotion_status = if matches!(signal_tier, SignalTier::Raw) {
        "raw"
    } else {
        "promoted"
    };

    #[allow(clippy::cast_possible_wrap)]
    let embedding_dim = embedding.as_ref().map(|e| e.len() as i64);
    let embedding_blob = embedding.map(|e| vector::vec_to_bytes(&e));

    EventMetadata {
        event_id,
        ingested_at,
        source,
        layer,
        privacy,
        event_type,
        signal_tier_str,
        source_kind,
        source_uri,
        provenance_source_class,
        provenance_reference,
        provenance_evidence_uri,
        retention_tier,
        retention_expires_at,
        content_type,
        contradiction_penalty,
        promotion_status,
        embedding_dim,
        embedding_blob,
    }
}

/// Decide whether the incoming event should replace the current belief-slot winner.
///
/// Returns `(should_replace, supersedes_event_id)`.
pub(super) async fn decide_replacement(
    pool: &SqlitePool,
    input: &MemoryEventInput,
) -> anyhow::Result<(bool, Option<String>)> {
    let current: Option<(String, String, f64, String)> = sqlx::query_as(
        "SELECT winner_event_id, source, confidence, updated_at
         FROM belief_slots
         WHERE entity_id = ?1 AND slot_key = ?2",
    )
    .bind(&input.entity_id)
    .bind(&input.slot_key)
    .fetch_optional(pool)
    .await
    .context("lookup incumbent belief slot")?;

    let should_replace = if let Some((
        _,
        ref current_source,
        current_confidence,
        ref current_updated_at,
    )) = current
    {
        let current_priority = codec::source_priority(codec::str_to_source(current_source));
        let incoming_priority = codec::source_priority(input.source);
        match incoming_priority.cmp(&current_priority) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => match input.confidence.total_cmp(&current_confidence) {
                Ordering::Greater => true,
                Ordering::Less => false,
                Ordering::Equal => {
                    matches!(
                        codec::compare_normalized_timestamps(
                            &input.occurred_at,
                            current_updated_at,
                        ),
                        Ordering::Greater
                    )
                }
            },
        }
    } else {
        true
    };

    let supersedes_event_id = current.and_then(|(winner_event_id, _, _, _)| {
        if should_replace || matches!(input.event_type, MemoryEventType::ContradictionMarked) {
            Some(winner_event_id)
        } else {
            None
        }
    });

    Ok((should_replace, supersedes_event_id))
}

/// Insert the event row, update the contradiction penalty, upsert belief slot
/// and retrieval unit if `should_replace`, and attempt raw-to-candidate promotion.
#[allow(clippy::too_many_lines)]
pub(super) async fn insert_event_records(
    pool: &SqlitePool,
    input: &MemoryEventInput,
    meta: &EventMetadata,
    should_replace: bool,
    supersedes_event_id: Option<&str>,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await.context("begin insert transaction")?;

    // 1. Insert into memory_events
    sqlx::query(
        "INSERT INTO memory_events (
            event_id, entity_id, slot_key, layer, event_type, value, source,
            confidence, importance, provenance_source_class, provenance_reference,
            provenance_evidence_uri, retention_tier, retention_expires_at,
            signal_tier, source_kind,
            privacy_level, occurred_at, ingested_at, supersedes_event_id
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
    )
    .bind(&meta.event_id)
    .bind(&input.entity_id)
    .bind(&input.slot_key)
    .bind(meta.layer)
    .bind(&meta.event_type)
    .bind(&input.value)
    .bind(meta.source)
    .bind(input.confidence)
    .bind(input.importance)
    .bind(meta.provenance_source_class)
    .bind(&meta.provenance_reference)
    .bind(&meta.provenance_evidence_uri)
    .bind(meta.retention_tier)
    .bind(&meta.retention_expires_at)
    .bind(meta.signal_tier_str)
    .bind(meta.source_kind)
    .bind(meta.privacy)
    .bind(&input.occurred_at)
    .bind(&meta.ingested_at)
    .bind(supersedes_event_id)
    .execute(&mut *tx)
    .await
    .context("insert memory event")?;

    // 2. Update contradiction penalty on existing retrieval unit
    if meta.contradiction_penalty > 0.0 {
        let unit_id = format!("{}:{}", input.entity_id, input.slot_key);
        sqlx::query(
            "UPDATE retrieval_units
             SET contradiction_penalty = MIN(1.0, contradiction_penalty + ?2)
             WHERE unit_id = ?1",
        )
        .bind(&unit_id)
        .bind(meta.contradiction_penalty)
        .execute(&mut *tx)
        .await
        .context("update contradiction penalty")?;
    }

    // 3. Upsert belief slot and retrieval unit if this event wins
    if should_replace {
        sqlx::query(
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
        )
        .bind(&input.entity_id)
        .bind(&input.slot_key)
        .bind(&input.value)
        .bind(&meta.event_id)
        .bind(meta.source)
        .bind(input.confidence)
        .bind(input.importance)
        .bind(meta.privacy)
        .bind(&input.occurred_at)
        .execute(&mut *tx)
        .await
        .context("upsert belief slot")?;

        let unit_id = format!("{}:{}", input.entity_id, input.slot_key);
        sqlx::query(
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
        )
        .bind(&unit_id)
        .bind(&input.entity_id)
        .bind(&input.slot_key)
        .bind(&input.value)
        .bind(meta.content_type)
        .bind(meta.signal_tier_str)
        .bind(meta.promotion_status)
        .bind(&meta.source_uri)
        .bind(meta.source_kind)
        .bind(input.importance)
        .bind(input.confidence)
        .bind(meta.contradiction_penalty)
        .bind(meta.privacy)
        .bind(&meta.embedding_blob)
        .bind(meta.embedding_dim)
        .bind(meta.layer)
        .bind(meta.provenance_source_class)
        .bind(&meta.provenance_reference)
        .bind(&meta.provenance_evidence_uri)
        .bind(meta.retention_tier)
        .bind(&meta.retention_expires_at)
        .bind(&input.occurred_at)
        .execute(&mut *tx)
        .await
        .context("upsert retrieval unit")?;

        // Promote raw signals that now have corroboration
        try_promote_raw_to_candidate(&mut tx, &input.entity_id, &input.slot_key)
            .await
            .context("promote corroborated raw signal")?;
    }

    tx.commit().await.context("commit insert transaction")?;
    Ok(())
}

/// If a slot has events from >=2 distinct sources, promote its retrieval unit
/// from `raw` to `candidate`.
async fn try_promote_raw_to_candidate(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entity_id: &str,
    slot_key: &str,
) -> anyhow::Result<()> {
    let (distinct_sources,): (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT source) FROM memory_events
         WHERE entity_id = ?1 AND slot_key = ?2",
    )
    .bind(entity_id)
    .bind(slot_key)
    .fetch_one(&mut **tx)
    .await?;

    if distinct_sources >= 2 {
        let unit_id = format!("{entity_id}:{slot_key}");
        sqlx::query(
            "UPDATE retrieval_units
             SET promotion_status = 'candidate'
             WHERE unit_id = ?1 AND promotion_status = 'raw'",
        )
        .bind(&unit_id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// Construct the `MemoryEvent` return value from input + metadata.
pub(super) fn build_event_output(input: &MemoryEventInput, meta: &EventMetadata) -> MemoryEvent {
    MemoryEvent {
        event_id: meta.event_id.clone(),
        entity_id: input.entity_id.clone(),
        slot_key: input.slot_key.clone(),
        event_type: input.event_type.clone(),
        value: input.value.clone(),
        source: input.source,
        confidence: input.confidence,
        importance: input.importance,
        provenance: input.provenance.clone(),
        privacy_level: input.privacy_level.clone(),
        occurred_at: input.occurred_at.clone(),
        ingested_at: meta.ingested_at.clone(),
    }
}
