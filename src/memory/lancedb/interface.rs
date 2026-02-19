use super::super::traits::{
    BeliefSlot, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, MemoryCategory, MemoryEvent,
    MemoryEventInput, MemoryLayer, MemoryProvenance, MemoryRecallItem, MemorySource, PrivacyLevel,
    RecallQuery,
};
use super::super::vector;
use super::batch::parse_entries;
use super::{
    LanceDbMemory, ProjectionEntry, StoredRow, EMBEDDING_STATUS_PENDING, EMBEDDING_STATUS_READY,
    LANCEDB_DEGRADED_SOFT_FORGET_MARKER, LANCEDB_DEGRADED_SOFT_FORGET_PROVENANCE,
    LANCEDB_DEGRADED_TOMBSTONE_MARKER, LANCEDB_DEGRADED_TOMBSTONE_PROVENANCE,
};

use chrono::Local;
use futures_util::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use std::collections::HashMap;
use uuid::Uuid;

impl LanceDbMemory {
    #[allow(clippy::unused_self)]
    pub(super) fn name(&self) -> &str {
        "lancedb"
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn upsert_projection_entry(
        &self,
        key: &str,
        content: &str,
        category: MemoryCategory,
        source: MemorySource,
        confidence: f64,
        importance: f64,
        privacy_level: PrivacyLevel,
        occurred_at: &str,
        layer: MemoryLayer,
        provenance: Option<MemoryProvenance>,
    ) -> anyhow::Result<()> {
        let now = Local::now().to_rfc3339();
        let cat = Self::category_to_str(&category);
        let source = Self::source_to_str(&source).to_string();
        let privacy_level = Self::privacy_to_str(&privacy_level).to_string();
        let layer = Self::layer_to_str(&layer).to_string();

        let (provenance_source_class, provenance_reference, provenance_evidence_uri) =
            if let Some(provenance) = provenance {
                (
                    Some(Self::source_to_str(&provenance.source_class).to_string()),
                    Some(provenance.reference),
                    provenance.evidence_uri,
                )
            } else {
                (None, None, None)
            };

        let existing = self.get_row_by_key(key).await?;
        let (id, created_at) = if let Some(ref row) = existing {
            (row.id.clone(), row.created_at.clone())
        } else {
            (Uuid::new_v4().to_string(), now.clone())
        };

        match category {
            MemoryCategory::Core => {
                let embedding = self
                    .inner
                    .embedder
                    .embed_one(content)
                    .await
                    .context("embedding failed")?;

                let row = StoredRow {
                    id,
                    key: key.to_string(),
                    content: content.to_string(),
                    category: cat.clone(),
                    source: source.clone(),
                    confidence,
                    importance,
                    privacy_level: privacy_level.clone(),
                    occurred_at: occurred_at.to_string(),
                    layer: layer.clone(),
                    provenance_source_class: provenance_source_class.clone(),
                    provenance_reference: provenance_reference.clone(),
                    provenance_evidence_uri: provenance_evidence_uri.clone(),
                    created_at: created_at.clone(),
                    updated_at: now.clone(),
                    embedding_status: EMBEDDING_STATUS_READY.to_string(),
                };
                self.upsert_row(&row, Some(&embedding)).await
            }
            MemoryCategory::Daily | MemoryCategory::Conversation | MemoryCategory::Custom(_) => {
                let row = StoredRow {
                    id,
                    key: key.to_string(),
                    content: content.to_string(),
                    category: cat,
                    source,
                    confidence,
                    importance,
                    privacy_level,
                    occurred_at: occurred_at.to_string(),
                    layer,
                    provenance_source_class,
                    provenance_reference,
                    provenance_evidence_uri,
                    created_at,
                    updated_at: now,
                    embedding_status: EMBEDDING_STATUS_PENDING.to_string(),
                };
                self.upsert_row(&row, None).await?;
                self.enqueue_backfill(key);
                Ok(())
            }
        }
    }

    pub(super) async fn search_projection(
        &self,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ProjectionEntry>> {
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut entries: HashMap<String, ProjectionEntry> = HashMap::new();

        let keyword = self
            .fts_search(query, limit.saturating_mul(2), &mut entries)
            .await
            .unwrap_or_else(|e| {
                tracing::debug!("lancedb fts search failed: {e}");
                Vec::new()
            });

        let query_embedding = self.inner.embedder.embed_one(query).await?;
        let vector = self
            .vector_search(&query_embedding, limit.saturating_mul(2), &mut entries)
            .await
            .unwrap_or_else(|e| {
                tracing::debug!("lancedb vector search failed: {e}");
                Vec::new()
            });

        if keyword.is_empty() && vector.is_empty() {
            return Ok(Vec::new());
        }

        let merged = if vector.is_empty() {
            let max_kw = keyword.iter().map(|(_, s)| *s).fold(0.0_f32, f32::max);
            let denom = if max_kw < f32::EPSILON { 1.0 } else { max_kw };
            let mut out = keyword
                .into_iter()
                .map(|(id, s)| vector::ScoredResult {
                    id,
                    vector_score: None,
                    keyword_score: Some(s / denom),
                    final_score: s / denom,
                })
                .collect::<Vec<_>>();
            out.sort_by(|a, b| {
                b.final_score
                    .partial_cmp(&a.final_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            out.truncate(limit);
            out
        } else {
            vector::hybrid_merge(
                &vector,
                &keyword,
                self.inner.vector_weight,
                self.inner.keyword_weight,
                limit,
            )
        };

        let mut results = Vec::new();
        for scored in merged {
            if let Some(mut entry) = entries.remove(&scored.id) {
                entry.score = Some(f64::from(scored.final_score));
                results.push(entry);
            }
        }
        results.truncate(limit);
        Ok(results)
    }

    pub(super) async fn fetch_projection_entry(
        &self,
        key: &str,
    ) -> anyhow::Result<Option<ProjectionEntry>> {
        let row = self.get_row_by_key(key).await?;
        Ok(row.map(|r| ProjectionEntry {
            id: r.id,
            key: r.key,
            content: r.content,
            category: Self::str_to_category(&r.category),
            timestamp: r.updated_at,
            source: Self::str_to_source(&r.source),
            confidence: r.confidence,
            importance: r.importance,
            privacy_level: Self::str_to_privacy(&r.privacy_level),
            occurred_at: r.occurred_at,
            score: None,
        }))
    }

    pub(super) async fn list_projection_entries(
        &self,
        category: Option<&MemoryCategory>,
    ) -> anyhow::Result<Vec<ProjectionEntry>> {
        let table = self.inner.table().await?;
        let mut q = table.query().select(Select::columns(&[
            "id",
            "key",
            "content",
            "category",
            "source",
            "confidence",
            "importance",
            "privacy_level",
            "occurred_at",
            "created_at",
        ]));
        if let Some(cat) = category {
            q = q.only_if(Self::sql_eq("category", &Self::category_to_str(cat)));
        }
        let mut stream = q.execute().await?;
        let mut out = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            out.extend(parse_entries(&batch));
        }
        Ok(out)
    }

    pub(super) async fn delete_projection_entry(&self, key: &str) -> anyhow::Result<bool> {
        if self.fetch_projection_entry(key).await?.is_none() {
            return Ok(false);
        }
        let table = self.inner.table().await?;
        let predicate = Self::sql_eq("key", key);
        table
            .delete(&predicate)
            .await
            .context("LanceDB delete failed")?;
        Ok(true)
    }

    pub(super) async fn count_projection_entries(&self) -> anyhow::Result<usize> {
        let table = self.inner.table().await?;
        let count = table.count_rows(None).await?;
        Ok(count)
    }

    pub(super) async fn health_check(&self) -> bool {
        match self.inner.table().await {
            Ok(t) => t.count_rows(None).await.is_ok(),
            Err(_) => false,
        }
    }

    pub(super) async fn append_event(
        &self,
        input: MemoryEventInput,
    ) -> anyhow::Result<MemoryEvent> {
        let input = input.normalize_for_ingress()?;
        let key = format!("{}:{}", input.entity_id, input.slot_key);
        self.upsert_projection_entry(
            &key,
            &input.value,
            Self::category_from_source(&input.source),
            input.source,
            input.confidence,
            input.importance,
            input.privacy_level.clone(),
            &input.occurred_at,
            input.layer,
            input.provenance.clone(),
        )
        .await?;

        Ok(MemoryEvent {
            event_id: Uuid::new_v4().to_string(),
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
            ingested_at: Local::now().to_rfc3339(),
        })
    }

    pub(super) async fn recall_scoped(
        &self,
        query: RecallQuery,
    ) -> anyhow::Result<Vec<MemoryRecallItem>> {
        query.enforce_policy()?;

        let scoped_query = format!("{} {}", query.entity_id, query.query);
        let entries = self.search_projection(&scoped_query, query.limit).await?;
        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                let (entity, slot) = entry.key.split_once(':')?;
                if entity != query.entity_id {
                    return None;
                }
                let base_score = entry.score.unwrap_or(0.0).clamp(0.0, 1.0);
                let final_score = 0.35_f64 * base_score
                    + 0.25_f64 * base_score
                    + 0.20_f64
                    + 0.10_f64 * 0.5
                    + 0.10_f64 * 0.8;
                Some(MemoryRecallItem {
                    entity_id: entity.to_string(),
                    slot_key: slot.to_string(),
                    value: entry.content,
                    source: entry.source,
                    confidence: entry.confidence,
                    importance: entry.importance,
                    privacy_level: entry.privacy_level,
                    score: final_score,
                    occurred_at: entry.occurred_at,
                })
            })
            .collect())
    }

    pub(super) async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        let key = format!("{entity_id}:{slot_key}");
        let row = self.fetch_projection_entry(&key).await?;
        Ok(row.map(|entry| BeliefSlot {
            entity_id: entity_id.to_string(),
            slot_key: slot_key.to_string(),
            value: entry.content,
            source: entry.source,
            confidence: entry.confidence,
            importance: entry.importance,
            privacy_level: entry.privacy_level,
            updated_at: entry.timestamp,
        }))
    }

    pub(super) async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        _reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        let key = format!("{entity_id}:{slot_key}");
        let degraded = matches!(mode, ForgetMode::Soft | ForgetMode::Tombstone);
        let applied = match mode {
            ForgetMode::Hard => self.delete_projection_entry(&key).await?,
            ForgetMode::Soft => {
                self.upsert_projection_entry(
                    &key,
                    LANCEDB_DEGRADED_SOFT_FORGET_MARKER,
                    MemoryCategory::Custom("degraded_soft_deleted".to_string()),
                    MemorySource::System,
                    0.0,
                    0.0,
                    PrivacyLevel::Private,
                    &Local::now().to_rfc3339(),
                    MemoryLayer::Working,
                    Some(MemoryProvenance::source_reference(
                        MemorySource::System,
                        LANCEDB_DEGRADED_SOFT_FORGET_PROVENANCE,
                    )),
                )
                .await?;
                true
            }
            ForgetMode::Tombstone => {
                self.upsert_projection_entry(
                    &key,
                    LANCEDB_DEGRADED_TOMBSTONE_MARKER,
                    MemoryCategory::Custom("degraded_tombstoned".to_string()),
                    MemorySource::System,
                    0.0,
                    0.0,
                    PrivacyLevel::Private,
                    &Local::now().to_rfc3339(),
                    MemoryLayer::Working,
                    Some(MemoryProvenance::source_reference(
                        MemorySource::System,
                        LANCEDB_DEGRADED_TOMBSTONE_PROVENANCE,
                    )),
                )
                .await?;
                true
            }
        };

        let slot_observed = if self.resolve_slot(entity_id, slot_key).await?.is_some() {
            ForgetArtifactObservation::PresentRetrievable
        } else {
            ForgetArtifactObservation::Absent
        };

        let projection_observed = if self.fetch_projection_entry(&key).await?.is_some() {
            ForgetArtifactObservation::PresentRetrievable
        } else {
            ForgetArtifactObservation::Absent
        };

        let slot_requirement = match mode {
            ForgetMode::Hard => ForgetArtifactRequirement::MustBeAbsent,
            ForgetMode::Soft | ForgetMode::Tombstone => {
                ForgetArtifactRequirement::MustBeNonRetrievable
            }
        };

        let projection_requirement = match mode {
            ForgetMode::Hard => ForgetArtifactRequirement::MustBeAbsent,
            ForgetMode::Soft | ForgetMode::Tombstone => {
                ForgetArtifactRequirement::MustBeNonRetrievable
            }
        };

        let artifact_checks = vec![
            ForgetArtifactCheck::new(ForgetArtifact::Slot, slot_requirement, slot_observed),
            ForgetArtifactCheck::new(
                ForgetArtifact::RetrievalDocs,
                ForgetArtifactRequirement::NotGoverned,
                ForgetArtifactObservation::Absent,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::ProjectionDocs,
                projection_requirement,
                projection_observed,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::Caches,
                ForgetArtifactRequirement::NotGoverned,
                ForgetArtifactObservation::Absent,
            ),
            ForgetArtifactCheck::new(
                ForgetArtifact::Ledger,
                ForgetArtifactRequirement::NotGoverned,
                ForgetArtifactObservation::Absent,
            ),
        ];

        Ok(ForgetOutcome::from_checks(
            entity_id,
            slot_key,
            mode,
            applied,
            degraded,
            artifact_checks,
        ))
    }

    pub(super) async fn count_events(
        &self,
        entity_id: Option<&str>,
    ) -> anyhow::Result<usize> {
        if let Some(entity) = entity_id {
            let entries = self.list_projection_entries(None).await?;
            let prefix = format!("{entity}:");
            Ok(entries
                .iter()
                .filter(|entry| entry.key.starts_with(&prefix))
                .count())
        } else {
            self.count_projection_entries().await
        }
    }
}

use anyhow::Context;
