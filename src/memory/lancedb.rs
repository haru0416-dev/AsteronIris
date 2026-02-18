use super::embeddings::EmbeddingProvider;
use super::traits::{
    BeliefSlot, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, Memory, MemoryCategory, MemoryEvent,
    MemoryEventInput, MemoryLayer, MemoryProvenance, MemoryRecallItem, MemorySource, PrivacyLevel,
    RecallQuery,
};
use super::vector;

use anyhow::Context;
use async_trait::async_trait;
use chrono::Local;

use arrow_array::builder::{FixedSizeListBuilder, Float32Builder};
use arrow_array::{Array, Float64Array, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};

use lancedb::index::scalar::FtsIndexBuilder;
use lancedb::index::Index;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::Table;
use tokio::sync::{mpsc, OnceCell};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

use futures_util::TryStreamExt;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const TABLE_NAME: &str = "memories";
const BACKFILL_QUEUE_CAPACITY: usize = 100;
const MAX_BACKFILL_RETRIES: u32 = 5;
const BASE_BACKOFF_MS: u64 = 200;
const MAX_BACKOFF_MS: u64 = 30_000;

const EMBEDDING_STATUS_READY: &str = "ready";
const EMBEDDING_STATUS_PENDING: &str = "pending";
const EMBEDDING_STATUS_FAILED: &str = "failed";

const LANCEDB_DEGRADED_SOFT_FORGET_MARKER: &str = "__LANCEDB_DEGRADED_SOFT_FORGET_MARKER__";
const LANCEDB_DEGRADED_TOMBSTONE_MARKER: &str = "__LANCEDB_DEGRADED_TOMBSTONE_MARKER__";
const LANCEDB_DEGRADED_SOFT_FORGET_PROVENANCE: &str = "lancedb:degraded:soft_forget_marker_rewrite";
const LANCEDB_DEGRADED_TOMBSTONE_PROVENANCE: &str = "lancedb:degraded:tombstone_marker_rewrite";

const LANCE_SCORE_COL: &str = "_score";
const LANCE_DISTANCE_COL: &str = "_distance";

#[derive(Debug, Clone)]
struct BackfillJob {
    key: String,
}

#[derive(Debug, Clone)]
struct StoredRow {
    id: String,
    key: String,
    content: String,
    category: String,
    source: String,
    confidence: f64,
    importance: f64,
    privacy_level: String,
    occurred_at: String,
    layer: String,
    provenance_source_class: Option<String>,
    provenance_reference: Option<String>,
    provenance_evidence_uri: Option<String>,
    created_at: String,
    updated_at: String,
    embedding_status: String,
}

#[derive(Debug, Clone)]
struct ProjectionEntry {
    id: String,
    key: String,
    content: String,
    category: MemoryCategory,
    timestamp: String,
    source: MemorySource,
    confidence: f64,
    importance: f64,
    privacy_level: PrivacyLevel,
    occurred_at: String,
    score: Option<f64>,
}

struct LanceDbInner {
    db_dir: PathBuf,
    schema: SchemaRef,
    embedder: Arc<dyn EmbeddingProvider>,
    vector_weight: f32,
    keyword_weight: f32,
    table: OnceCell<Table>,
}

impl LanceDbInner {
    async fn table(&self) -> anyhow::Result<&Table> {
        self.table
            .get_or_try_init(|| async {
                let uri = self.db_dir.to_string_lossy().to_string();
                let conn = lancedb::connect(&uri)
                    .execute()
                    .await
                    .with_context(|| format!("Failed to connect to LanceDB at {uri}"))?;

                let table = match conn.open_table(TABLE_NAME).execute().await {
                    Ok(t) => t,
                    Err(_) => conn
                        .create_empty_table(TABLE_NAME, self.schema.clone())
                        .execute()
                        .await
                        .context("Failed to create empty LanceDB memories table")?,
                };

                if let Err(e) = table
                    .create_index(&["content"], Index::FTS(FtsIndexBuilder::default()))
                    .execute()
                    .await
                {
                    tracing::debug!("lancedb fts index create skipped: {e}");
                }

                if let Err(e) = table
                    .create_index(&["embedding"], Index::Auto)
                    .execute()
                    .await
                {
                    tracing::debug!("lancedb vector index create skipped: {e}");
                }

                Ok(table)
            })
            .await
    }
}

pub struct LanceDbMemory {
    inner: Arc<LanceDbInner>,
    backfill_tx: mpsc::Sender<BackfillJob>,
    backfill_worker: JoinHandle<()>,
}

#[allow(
    clippy::unused_self,
    clippy::unused_async,
    clippy::trivially_copy_pass_by_ref
)]
impl LanceDbMemory {
    pub fn with_embedder(
        workspace_dir: &Path,
        embedder: Arc<dyn EmbeddingProvider>,
        vector_weight: f32,
        keyword_weight: f32,
    ) -> anyhow::Result<Self> {
        let dims = embedder.dimensions();
        anyhow::ensure!(
            dims > 0,
            "LanceDB memory backend requires embeddings (embedding_dimensions > 0)"
        );

        let db_dir = workspace_dir.join("memory").join("lancedb");
        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("Failed to create LanceDB dir: {}", db_dir.display()))?;

        let dims_i32 =
            i32::try_from(dims).with_context(|| format!("Invalid embedding dimension: {dims}"))?;
        anyhow::ensure!(dims_i32 > 0, "Invalid embedding dimension: {dims}");

        let embedding_field = Field::new("item", DataType::Float32, true);
        let embedding_dt = DataType::FixedSizeList(Arc::new(embedding_field), dims_i32);
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("key", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("category", DataType::Utf8, false),
            Field::new("source", DataType::Utf8, false),
            Field::new("confidence", DataType::Float64, false),
            Field::new("importance", DataType::Float64, false),
            Field::new("privacy_level", DataType::Utf8, false),
            Field::new("occurred_at", DataType::Utf8, false),
            Field::new("layer", DataType::Utf8, false),
            Field::new("provenance_source_class", DataType::Utf8, true),
            Field::new("provenance_reference", DataType::Utf8, true),
            Field::new("provenance_evidence_uri", DataType::Utf8, true),
            Field::new("created_at", DataType::Utf8, false),
            Field::new("updated_at", DataType::Utf8, false),
            Field::new("embedding", embedding_dt, true),
            Field::new("embedding_status", DataType::Utf8, false),
        ]));

        let inner = Arc::new(LanceDbInner {
            db_dir,
            schema,
            embedder,
            vector_weight,
            keyword_weight,
            table: OnceCell::new(),
        });

        let (tx, rx) = mpsc::channel(BACKFILL_QUEUE_CAPACITY);
        let worker_inner = inner.clone();
        let worker = tokio::spawn(async move {
            run_backfill_worker(worker_inner, rx).await;
        });

        Ok(Self {
            inner,
            backfill_tx: tx,
            backfill_worker: worker,
        })
    }

    fn category_to_str(cat: &MemoryCategory) -> String {
        match cat {
            MemoryCategory::Core => "core".into(),
            MemoryCategory::Daily => "daily".into(),
            MemoryCategory::Conversation => "conversation".into(),
            MemoryCategory::Custom(name) => name.clone(),
        }
    }

    fn str_to_category(s: &str) -> MemoryCategory {
        match s {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }

    fn sql_eq(column: &str, value: &str) -> String {
        let v = value.replace('\'', "''");
        format!("{column} = '{v}'")
    }

    fn source_from_category(category: &MemoryCategory) -> MemorySource {
        match category {
            MemoryCategory::Core => MemorySource::ExplicitUser,
            MemoryCategory::Daily => MemorySource::System,
            MemoryCategory::Conversation => MemorySource::Inferred,
            MemoryCategory::Custom(_) => MemorySource::ToolVerified,
        }
    }

    fn source_to_str(source: &MemorySource) -> &'static str {
        match source {
            MemorySource::ExplicitUser => "explicit_user",
            MemorySource::ToolVerified => "tool_verified",
            MemorySource::System => "system",
            MemorySource::Inferred => "inferred",
        }
    }

    fn str_to_source(source: &str) -> MemorySource {
        match source {
            "explicit_user" => MemorySource::ExplicitUser,
            "tool_verified" => MemorySource::ToolVerified,
            "inferred" => MemorySource::Inferred,
            _ => MemorySource::System,
        }
    }

    fn privacy_to_str(level: &PrivacyLevel) -> &'static str {
        match level {
            PrivacyLevel::Public => "public",
            PrivacyLevel::Private => "private",
            PrivacyLevel::Secret => "secret",
        }
    }

    fn str_to_privacy(level: &str) -> PrivacyLevel {
        match level {
            "public" => PrivacyLevel::Public,
            "secret" => PrivacyLevel::Secret,
            _ => PrivacyLevel::Private,
        }
    }

    fn layer_to_str(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    fn category_from_source(source: &MemorySource) -> MemoryCategory {
        match source {
            MemorySource::ExplicitUser | MemorySource::ToolVerified => MemoryCategory::Core,
            MemorySource::System => MemoryCategory::Daily,
            MemorySource::Inferred => MemoryCategory::Conversation,
        }
    }

    async fn get_row_by_key(&self, key: &str) -> anyhow::Result<Option<StoredRow>> {
        let table = self.inner.table().await?;
        let filter = Self::sql_eq("key", key);
        let mut stream = table
            .query()
            .only_if(filter)
            .limit(1)
            .select(Select::columns(&[
                "id",
                "key",
                "content",
                "category",
                "source",
                "confidence",
                "importance",
                "privacy_level",
                "occurred_at",
                "layer",
                "provenance_source_class",
                "provenance_reference",
                "provenance_evidence_uri",
                "created_at",
                "updated_at",
                "embedding_status",
            ]))
            .execute()
            .await
            .context("LanceDB get query failed")?;

        let mut out = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            out.extend(parse_rows(&batch));
            if !out.is_empty() {
                break;
            }
        }
        Ok(out.into_iter().next())
    }

    async fn upsert_row(&self, row: &StoredRow, embedding: Option<&[f32]>) -> anyhow::Result<()> {
        let table = self.inner.table().await?;
        let batch = build_row_batch(self.inner.schema.clone(), row, embedding)?;

        let schema = batch.schema();
        let reader = RecordBatchIterator::new([Ok(batch)].into_iter(), schema);

        let mut merge_insert = table.merge_insert(&["key"]);
        merge_insert
            .when_matched_update_all(None)
            .when_not_matched_insert_all();
        merge_insert
            .execute(Box::new(reader))
            .await
            .context("LanceDB merge_insert failed")?;
        Ok(())
    }

    fn enqueue_backfill(&self, key: &str) {
        let job = BackfillJob {
            key: key.to_string(),
        };
        if let Err(_e) = self.backfill_tx.try_send(job) {
            tracing::warn!("lancedb backfill queue full; dropping job");
        }
    }

    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        entries: &mut HashMap<String, ProjectionEntry>,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        use lancedb::index::scalar::FullTextSearchQuery;

        let table = self.inner.table().await?;
        let mut stream = table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_string()))
            .limit(limit)
            .select(Select::columns(&[
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
                LANCE_SCORE_COL,
            ]))
            .execute()
            .await
            .context("LanceDB full_text_search failed")?;

        let mut scored = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            let (rows, row_scores) = parse_entries_and_score(&batch, LANCE_SCORE_COL);
            for (row, score) in rows.into_iter().zip(row_scores.into_iter()) {
                entries.insert(row.id.clone(), row.clone());
                scored.push((row.id, score));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    async fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
        entries: &mut HashMap<String, ProjectionEntry>,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        let table = self.inner.table().await?;
        let mut stream = table
            .query()
            .only_if(Self::sql_eq("embedding_status", EMBEDDING_STATUS_READY))
            .nearest_to(query_embedding)?
            .column("embedding")
            .distance_type(lancedb::DistanceType::Cosine)
            .limit(limit)
            .select(Select::columns(&[
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
                LANCE_DISTANCE_COL,
            ]))
            .execute()
            .await
            .context("LanceDB vector search failed")?;

        let mut scored = Vec::new();
        while let Some(batch) = stream.try_next().await? {
            let (rows, dists) = parse_entries_and_score(&batch, LANCE_DISTANCE_COL);
            for (row, dist) in rows.into_iter().zip(dists.into_iter()) {
                entries.insert(row.id.clone(), row.clone());
                let sim = (1.0 - dist).clamp(0.0, 1.0);
                scored.push((row.id, sim));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }
}

impl Drop for LanceDbMemory {
    fn drop(&mut self) {
        self.backfill_worker.abort();
    }
}

impl LanceDbMemory {
    #[allow(clippy::unused_self)]
    fn name(&self) -> &str {
        "lancedb"
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_projection_entry(
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

    async fn search_projection(
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

    async fn fetch_projection_entry(&self, key: &str) -> anyhow::Result<Option<ProjectionEntry>> {
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

    async fn list_projection_entries(
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

    async fn delete_projection_entry(&self, key: &str) -> anyhow::Result<bool> {
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

    async fn count_projection_entries(&self) -> anyhow::Result<usize> {
        let table = self.inner.table().await?;
        let count = table.count_rows(None).await?;
        Ok(count)
    }

    async fn health_check(&self) -> bool {
        match self.inner.table().await {
            Ok(t) => t.count_rows(None).await.is_ok(),
            Err(_) => false,
        }
    }

    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
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

    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
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

    async fn resolve_slot(
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

    async fn forget_slot(
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

    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        if let Some(entity) = entity_id {
            let entries = self.list_projection_entries(None).await?;
            Ok(entries
                .iter()
                .filter(|entry| entry.key.starts_with(&format!("{entity}:")))
                .count())
        } else {
            self.count_projection_entries().await
        }
    }
}

async fn run_backfill_worker(inner: Arc<LanceDbInner>, mut rx: mpsc::Receiver<BackfillJob>) {
    while let Some(job) = rx.recv().await {
        if let Err(e) = backfill_one(&inner, &job.key).await {
            let _ = e;
            tracing::debug!("lancedb backfill failed");
        }
    }
}

async fn backfill_one(inner: &LanceDbInner, key: &str) -> anyhow::Result<()> {
    let table = inner.table().await?;
    let filter = LanceDbMemory::sql_eq("key", key);

    let mut stream = table
        .query()
        .only_if(filter)
        .limit(1)
        .select(Select::columns(&[
            "id",
            "key",
            "content",
            "category",
            "source",
            "confidence",
            "importance",
            "privacy_level",
            "occurred_at",
            "layer",
            "provenance_source_class",
            "provenance_reference",
            "provenance_evidence_uri",
            "created_at",
            "updated_at",
            "embedding_status",
        ]))
        .execute()
        .await?;

    let mut row: Option<StoredRow> = None;
    while let Some(batch) = stream.try_next().await? {
        let mut rows = parse_rows(&batch);
        if let Some(r) = rows.pop() {
            row = Some(r);
            break;
        }
    }

    let Some(mut row) = row else {
        return Ok(());
    };

    if row.embedding_status == EMBEDDING_STATUS_READY {
        return Ok(());
    }

    let mut backoff_ms = BASE_BACKOFF_MS;
    for attempt in 0..=MAX_BACKFILL_RETRIES {
        let embed_res = inner.embedder.embed_one(&row.content).await;
        match embed_res {
            Ok(embedding) => {
                row.embedding_status = EMBEDDING_STATUS_READY.to_string();
                row.updated_at = Local::now().to_rfc3339();

                let batch = build_row_batch(inner.schema.clone(), &row, Some(&embedding))?;
                let schema = batch.schema();
                let reader = RecordBatchIterator::new([Ok(batch)].into_iter(), schema);
                let mut merge_insert = table.merge_insert(&["key"]);
                merge_insert
                    .when_matched_update_all(None)
                    .when_not_matched_insert_all();
                merge_insert.execute(Box::new(reader)).await?;
                return Ok(());
            }
            Err(e) => {
                if attempt >= MAX_BACKFILL_RETRIES {
                    let _ = e;
                    tracing::warn!("lancedb backfill exhausted retries for one item");
                    row.embedding_status = EMBEDDING_STATUS_FAILED.to_string();
                    row.updated_at = Local::now().to_rfc3339();
                    let batch = build_row_batch(inner.schema.clone(), &row, None)?;
                    let schema = batch.schema();
                    let reader = RecordBatchIterator::new([Ok(batch)].into_iter(), schema);
                    let mut merge_insert = table.merge_insert(&["key"]);
                    merge_insert
                        .when_matched_update_all(None)
                        .when_not_matched_insert_all();
                    merge_insert.execute(Box::new(reader)).await?;
                    return Ok(());
                }

                let jitter_ms = u64::from(chrono::Utc::now().timestamp_subsec_millis() % 250);
                sleep(Duration::from_millis(backoff_ms + jitter_ms)).await;
                backoff_ms = (backoff_ms.saturating_mul(2)).min(MAX_BACKOFF_MS);
            }
        }
    }

    Ok(())
}

fn build_row_batch(
    schema: SchemaRef,
    row: &StoredRow,
    embedding: Option<&[f32]>,
) -> anyhow::Result<RecordBatch> {
    let id = Arc::new(StringArray::from(vec![Some(row.id.as_str())]));
    let key = Arc::new(StringArray::from(vec![Some(row.key.as_str())]));
    let content = Arc::new(StringArray::from(vec![Some(row.content.as_str())]));
    let category = Arc::new(StringArray::from(vec![Some(row.category.as_str())]));
    let source = Arc::new(StringArray::from(vec![Some(row.source.as_str())]));
    let confidence = Arc::new(Float64Array::from(vec![row.confidence]));
    let importance = Arc::new(Float64Array::from(vec![row.importance]));
    let privacy_level = Arc::new(StringArray::from(vec![Some(row.privacy_level.as_str())]));
    let occurred_at = Arc::new(StringArray::from(vec![Some(row.occurred_at.as_str())]));
    let layer = Arc::new(StringArray::from(vec![Some(row.layer.as_str())]));
    let provenance_source_class = Arc::new(StringArray::from(vec![row
        .provenance_source_class
        .as_deref()]));
    let provenance_reference =
        Arc::new(StringArray::from(vec![row.provenance_reference.as_deref()]));
    let provenance_evidence_uri = Arc::new(StringArray::from(vec![row
        .provenance_evidence_uri
        .as_deref()]));
    let created_at = Arc::new(StringArray::from(vec![Some(row.created_at.as_str())]));
    let updated_at = Arc::new(StringArray::from(vec![Some(row.updated_at.as_str())]));
    let status = Arc::new(StringArray::from(vec![Some(row.embedding_status.as_str())]));

    let dims = match schema.field_with_name("embedding")?.data_type() {
        DataType::FixedSizeList(_, n) => *n,
        other => anyhow::bail!("Unexpected embedding type in schema: {other:?}"),
    };

    let dims_usize = usize::try_from(dims)
        .with_context(|| format!("Invalid embedding dimension in schema: {dims}"))?;

    let mut emb_builder = FixedSizeListBuilder::new(Float32Builder::new(), dims);
    if let Some(v) = embedding {
        if v.len() != dims_usize {
            anyhow::bail!(
                "Embedding dimension mismatch: got {}, expected {}",
                v.len(),
                dims
            );
        }
        emb_builder.values().append_slice(v);
        emb_builder.append(true);
    } else {
        for _ in 0..dims_usize {
            emb_builder.values().append_value(0.0);
        }
        emb_builder.append(false);
    }
    let embedding_arr = Arc::new(emb_builder.finish());

    let cols: Vec<Arc<dyn Array>> = vec![
        id,
        key,
        content,
        category,
        source,
        confidence,
        importance,
        privacy_level,
        occurred_at,
        layer,
        provenance_source_class,
        provenance_reference,
        provenance_evidence_uri,
        created_at,
        updated_at,
        embedding_arr,
        status,
    ];
    Ok(RecordBatch::try_new(schema, cols)?)
}

#[allow(clippy::too_many_lines)]
fn parse_rows(batch: &RecordBatch) -> Vec<StoredRow> {
    let id = batch
        .column_by_name("id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let key = batch
        .column_by_name("key")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let content = batch
        .column_by_name("content")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let category = batch
        .column_by_name("category")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let source = batch
        .column_by_name("source")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let importance = batch
        .column_by_name("importance")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let privacy_level = batch
        .column_by_name("privacy_level")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let layer = batch
        .column_by_name("layer")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let provenance_source_class = batch
        .column_by_name("provenance_source_class")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let provenance_reference = batch
        .column_by_name("provenance_reference")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let provenance_evidence_uri = batch
        .column_by_name("provenance_evidence_uri")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let created_at = batch
        .column_by_name("created_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let updated_at = batch
        .column_by_name("updated_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let embedding_status = batch
        .column_by_name("embedding_status")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let (
        Some(id),
        Some(key),
        Some(content),
        Some(category),
        Some(created_at),
        Some(updated_at),
        Some(embedding_status),
    ) = (
        id,
        key,
        content,
        category,
        created_at,
        updated_at,
        embedding_status,
    )
    else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(batch.num_rows());
    for i in 0..batch.num_rows() {
        if id.is_null(i)
            || key.is_null(i)
            || content.is_null(i)
            || category.is_null(i)
            || created_at.is_null(i)
            || updated_at.is_null(i)
            || embedding_status.is_null(i)
        {
            continue;
        }

        out.push(StoredRow {
            id: id.value(i).to_string(),
            key: key.value(i).to_string(),
            content: content.value(i).to_string(),
            category: category.value(i).to_string(),
            source: source
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| {
                    LanceDbMemory::source_to_str(&LanceDbMemory::source_from_category(
                        &LanceDbMemory::str_to_category(category.value(i)),
                    ))
                    .to_string()
                }),
            confidence: confidence.map_or(0.0, |col| col.value(i)),
            importance: importance.map_or(0.0, |col| col.value(i)),
            privacy_level: privacy_level
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| "private".to_string()),
            occurred_at: occurred_at
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| created_at.value(i).to_string()),
            layer: layer
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| "working".to_string()),
            provenance_source_class: provenance_source_class
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string())),
            provenance_reference: provenance_reference
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string())),
            provenance_evidence_uri: provenance_evidence_uri
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string())),
            created_at: created_at.value(i).to_string(),
            updated_at: updated_at.value(i).to_string(),
            embedding_status: embedding_status.value(i).to_string(),
        });
    }
    out
}

fn parse_entries(batch: &RecordBatch) -> Vec<ProjectionEntry> {
    let id = batch
        .column_by_name("id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let key = batch
        .column_by_name("key")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let content = batch
        .column_by_name("content")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let category = batch
        .column_by_name("category")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let source = batch
        .column_by_name("source")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let importance = batch
        .column_by_name("importance")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let privacy_level = batch
        .column_by_name("privacy_level")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let created_at = batch
        .column_by_name("created_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let (Some(id), Some(key), Some(content), Some(category), Some(created_at)) =
        (id, key, content, category, created_at)
    else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(batch.num_rows());
    for i in 0..batch.num_rows() {
        if id.is_null(i)
            || key.is_null(i)
            || content.is_null(i)
            || category.is_null(i)
            || created_at.is_null(i)
        {
            continue;
        }

        let parsed_category = LanceDbMemory::str_to_category(category.value(i));
        let parsed_source = source
            .and_then(|col| (!col.is_null(i)).then(|| LanceDbMemory::str_to_source(col.value(i))))
            .unwrap_or_else(|| LanceDbMemory::source_from_category(&parsed_category));

        out.push(ProjectionEntry {
            id: id.value(i).to_string(),
            key: key.value(i).to_string(),
            content: content.value(i).to_string(),
            category: parsed_category,
            timestamp: created_at.value(i).to_string(),
            source: parsed_source,
            confidence: confidence.map_or(0.0, |col| col.value(i)),
            importance: importance.map_or(0.0, |col| col.value(i)),
            privacy_level: privacy_level
                .and_then(|col| {
                    (!col.is_null(i)).then(|| LanceDbMemory::str_to_privacy(col.value(i)))
                })
                .unwrap_or(PrivacyLevel::Private),
            occurred_at: occurred_at
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| created_at.value(i).to_string()),
            score: None,
        });
    }
    out
}

#[allow(clippy::cast_possible_truncation)]
fn parse_entries_and_score(
    batch: &RecordBatch,
    score_col: &str,
) -> (Vec<ProjectionEntry>, Vec<f32>) {
    let id = batch
        .column_by_name("id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let key = batch
        .column_by_name("key")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let content = batch
        .column_by_name("content")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let category = batch
        .column_by_name("category")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let source = batch
        .column_by_name("source")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let importance = batch
        .column_by_name("importance")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let privacy_level = batch
        .column_by_name("privacy_level")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let created_at = batch
        .column_by_name("created_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let (Some(id), Some(key), Some(content), Some(category), Some(created_at)) =
        (id, key, content, category, created_at)
    else {
        return (Vec::new(), Vec::new());
    };

    let col = batch.column_by_name(score_col);
    let f32s = col.and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>());
    let f64s = col.and_then(|c| c.as_any().downcast_ref::<arrow_array::Float64Array>());

    let mut entries_out = Vec::with_capacity(batch.num_rows());
    let mut scores_out = Vec::with_capacity(batch.num_rows());

    for i in 0..batch.num_rows() {
        if id.is_null(i)
            || key.is_null(i)
            || content.is_null(i)
            || category.is_null(i)
            || created_at.is_null(i)
        {
            continue;
        }

        let score = if let Some(a) = f32s {
            a.value(i)
        } else if let Some(a) = f64s {
            a.value(i) as f32
        } else {
            0.0
        };

        let parsed_category = LanceDbMemory::str_to_category(category.value(i));
        let parsed_source = source
            .and_then(|col| (!col.is_null(i)).then(|| LanceDbMemory::str_to_source(col.value(i))))
            .unwrap_or_else(|| LanceDbMemory::source_from_category(&parsed_category));

        entries_out.push(ProjectionEntry {
            id: id.value(i).to_string(),
            key: key.value(i).to_string(),
            content: content.value(i).to_string(),
            category: parsed_category,
            timestamp: created_at.value(i).to_string(),
            source: parsed_source,
            confidence: confidence.map_or(0.0, |col| col.value(i)),
            importance: importance.map_or(0.0, |col| col.value(i)),
            privacy_level: privacy_level
                .and_then(|col| {
                    (!col.is_null(i)).then(|| LanceDbMemory::str_to_privacy(col.value(i)))
                })
                .unwrap_or(PrivacyLevel::Private),
            occurred_at: occurred_at
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| created_at.value(i).to_string()),
            score: None,
        });
        scores_out.push(score);
    }

    (entries_out, scores_out)
}

#[async_trait]
impl Memory for LanceDbMemory {
    fn name(&self) -> &str {
        LanceDbMemory::name(self)
    }

    async fn health_check(&self) -> bool {
        LanceDbMemory::health_check(self).await
    }

    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
        LanceDbMemory::append_event(self, input).await
    }

    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
        LanceDbMemory::recall_scoped(self, query).await
    }

    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        LanceDbMemory::resolve_slot(self, entity_id, slot_key).await
    }

    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        LanceDbMemory::forget_slot(self, entity_id, slot_key, mode, reason).await
    }

    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        LanceDbMemory::count_events(self, entity_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn test_upsert(mem: &LanceDbMemory, key: &str, content: &str, category: MemoryCategory) {
        mem.upsert_projection_entry(
            key,
            content,
            category,
            MemorySource::ExplicitUser,
            0.95,
            0.5,
            PrivacyLevel::Private,
            "2026-01-01T00:00:00Z",
            MemoryLayer::Working,
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn lancedb_name_and_health() {
        let tmp = TempDir::new().unwrap();
        let embedder = Arc::new(super::super::embeddings::DeterministicEmbedding::new(8));
        let mem = LanceDbMemory::with_embedder(tmp.path(), embedder, 0.7, 0.3).unwrap();
        assert_eq!(mem.name(), "lancedb");
        assert!(mem.health_check().await);
    }

    #[tokio::test]
    async fn lancedb_store_get_upsert_and_recall() {
        let tmp = TempDir::new().unwrap();
        let embedder = Arc::new(super::super::embeddings::DeterministicEmbedding::new(8));
        let mem = LanceDbMemory::with_embedder(tmp.path(), embedder, 0.7, 0.3).unwrap();

        test_upsert(&mem, "core_k", "Rust is fast", MemoryCategory::Core).await;
        test_upsert(
            &mem,
            "daily_k",
            "Daily note about Rust",
            MemoryCategory::Daily,
        )
        .await;

        let core_row = mem.get_row_by_key("core_k").await.unwrap().unwrap();
        assert_eq!(core_row.embedding_status, EMBEDDING_STATUS_READY);

        let mut daily_status = None;
        for _ in 0..50 {
            let row = mem.get_row_by_key("daily_k").await.unwrap().unwrap();
            if row.embedding_status == EMBEDDING_STATUS_READY {
                daily_status = Some(row.embedding_status);
                break;
            }
            if row.embedding_status == EMBEDDING_STATUS_FAILED {
                daily_status = Some(row.embedding_status);
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(daily_status.as_deref(), Some(EMBEDDING_STATUS_READY));

        let core = mem.fetch_projection_entry("core_k").await.unwrap().unwrap();
        assert_eq!(core.content, "Rust is fast");
        assert_eq!(core.category, MemoryCategory::Core);

        test_upsert(&mem, "core_k", "Rust is very fast", MemoryCategory::Core).await;
        let core2 = mem.fetch_projection_entry("core_k").await.unwrap().unwrap();
        assert_eq!(core2.content, "Rust is very fast");

        let results = mem.search_projection("Rust", 10).await.unwrap();
        assert!(!results.is_empty());
        for r in &results {
            assert!(r.score.is_some());
        }
    }

    #[tokio::test]
    async fn lancedb_recall_respects_limit() {
        let tmp = TempDir::new().unwrap();
        let embedder = Arc::new(super::super::embeddings::DeterministicEmbedding::new(8));
        let mem = LanceDbMemory::with_embedder(tmp.path(), embedder, 0.7, 0.3).unwrap();

        for i in 0..20 {
            test_upsert(
                &mem,
                &format!("k{i}"),
                &format!("Rust item {i}"),
                MemoryCategory::Core,
            )
            .await;
        }

        let results = mem.search_projection("Rust", 3).await.unwrap();
        assert!(results.len() <= 3);
    }

    #[tokio::test]
    async fn lancedb_forget_and_count() {
        let tmp = TempDir::new().unwrap();
        let embedder = Arc::new(super::super::embeddings::DeterministicEmbedding::new(8));
        let mem = LanceDbMemory::with_embedder(tmp.path(), embedder, 0.7, 0.3).unwrap();

        assert_eq!(mem.count_projection_entries().await.unwrap(), 0);
        test_upsert(&mem, "a", "one", MemoryCategory::Core).await;
        test_upsert(&mem, "b", "two", MemoryCategory::Core).await;
        assert_eq!(mem.count_projection_entries().await.unwrap(), 2);

        assert!(mem.delete_projection_entry("a").await.unwrap());
        assert!(!mem.delete_projection_entry("a").await.unwrap());
        assert_eq!(mem.count_projection_entries().await.unwrap(), 1);
    }
}
