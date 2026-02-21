mod backfill;
mod batch;
mod conversions;
mod interface;
mod query;

use super::embeddings::EmbeddingProvider;
use super::traits::{
    BeliefSlot, ForgetMode, ForgetOutcome, Memory, MemoryCategory, MemoryEvent, MemoryEventInput,
    MemoryRecallItem, MemorySource, PrivacyLevel, RecallQuery,
};

use anyhow::Context;
use async_trait::async_trait;

use arrow_schema::{DataType, Field, Schema, SchemaRef};

use lancedb::Table;
use lancedb::index::Index;
use lancedb::index::scalar::FtsIndexBuilder;
use tokio::sync::{OnceCell, mpsc};
use tokio::task::JoinHandle;

use std::path::Path;
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
    #[allow(dead_code)] // Deserialized for test assertions in LanceDB projection flows
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
    db_dir: std::path::PathBuf,
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
                let uri = self.db_dir.to_string_lossy().into_owned();
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
            backfill::run_backfill_worker(worker_inner, rx).await;
        });

        Ok(Self {
            inner,
            backfill_tx: tx,
            backfill_worker: worker,
        })
    }
}

impl Drop for LanceDbMemory {
    fn drop(&mut self) {
        self.backfill_worker.abort();
    }
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
    use super::super::traits::MemoryLayer;
    use super::*;
    use tempfile::TempDir;
    use tokio::time::{Duration, sleep};

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
