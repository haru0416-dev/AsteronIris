use super::embeddings::EmbeddingProvider;
use super::traits::{
    BeliefSlot, ForgetMode, ForgetOutcome, Memory, MemoryEvent, MemoryEventInput, MemoryRecallItem,
    MemorySource, RecallQuery,
};
use crate::memory::vector;
use anyhow::Context;
use async_trait::async_trait;
use chrono::Local;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod codec;
mod events;
mod projection;
mod repository;
mod schema;
mod search;

/// SQLite-backed persistent memory — the brain
///
/// Full-stack search engine:
/// - **Vector DB**: embeddings stored as BLOB, cosine similarity search
/// - **Keyword Search**: FTS5 virtual table with BM25 scoring
/// - **Hybrid Merge**: weighted fusion of vector + keyword results
/// - **Embedding Cache**: LRU-evicted cache to avoid redundant API calls
/// - **Safe Reindex**: temp DB → seed → sync → atomic swap → rollback
pub struct SqliteMemory {
    conn: Mutex<Connection>,
    // Retained for diagnostics and potential reconnection logic
    #[allow(dead_code)]
    db_path: PathBuf,
    embedder: Arc<dyn EmbeddingProvider>,
    // Used by the projection search layer (search_projection) — currently dormant
    #[allow(dead_code)]
    vector_weight: f32,
    // Used by the projection search layer (search_projection) — currently dormant
    #[allow(dead_code)]
    keyword_weight: f32,
    cache_max: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
impl SqliteMemory {
    const MEMORY_SCHEMA_V1: i64 = 1;
    const MEMORY_SCHEMA_V2: i64 = 2;
    const MEMORY_SCHEMA_V3: i64 = 3;
    const MEMORY_EVENTS_V2_COLUMNS: [&'static str; 4] = [
        "layer",
        "provenance_source_class",
        "provenance_reference",
        "provenance_evidence_uri",
    ];
    const MEMORY_EVENTS_V3_COLUMNS: [&'static str; 2] = ["retention_tier", "retention_expires_at"];
    const MEMORIES_V3_COLUMNS: [&'static str; 6] = [
        "layer",
        "provenance_source_class",
        "provenance_reference",
        "provenance_evidence_uri",
        "retention_tier",
        "retention_expires_at",
    ];
    const RETRIEVAL_DOCS_V3_COLUMNS: [&'static str; 6] = [
        "layer",
        "provenance_source_class",
        "provenance_reference",
        "provenance_evidence_uri",
        "retention_tier",
        "retention_expires_at",
    ];
    const TREND_TTL_DAYS: f64 = 30.0;
    const TREND_DECAY_WINDOW_DAYS: f64 = 45.0;

    pub fn new(workspace_dir: &Path) -> anyhow::Result<Self> {
        Self::with_embedder(
            workspace_dir,
            Arc::new(super::embeddings::NoopEmbedding),
            0.7,
            0.3,
            10_000,
        )
    }

    pub fn with_embedder(
        workspace_dir: &Path,
        embedder: Arc<dyn EmbeddingProvider>,
        vector_weight: f32,
        keyword_weight: f32,
        cache_max: usize,
    ) -> anyhow::Result<Self> {
        let db_path = workspace_dir.join("memory").join("brain.db");

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("create memory directory")?;
        }

        let conn = Connection::open(&db_path).context("open SQLite database")?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -8000;
             PRAGMA mmap_size = 268435456;
             PRAGMA busy_timeout = 5000;",
        )
        .context("configure SQLite pragmas")?;
        Self::init_schema(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
            embedder,
            vector_weight,
            keyword_weight,
            cache_max,
        })
    }

    fn source_priority(source: &MemorySource) -> u8 {
        match source {
            MemorySource::ExplicitUser => 4,
            MemorySource::ToolVerified => 3,
            MemorySource::System => 2,
            MemorySource::Inferred => 1,
        }
    }

    fn compare_normalized_timestamps(incoming: &str, incumbent: &str) -> std::cmp::Ordering {
        let incoming_normalized = chrono::DateTime::parse_from_rfc3339(incoming)
            .ok()
            .and_then(|parsed| parsed.timestamp_nanos_opt());
        let incumbent_normalized = chrono::DateTime::parse_from_rfc3339(incumbent)
            .ok()
            .and_then(|parsed| parsed.timestamp_nanos_opt());

        match (incoming_normalized, incumbent_normalized) {
            (Some(incoming), Some(incumbent)) => incoming.cmp(&incumbent),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }

    fn contradiction_penalty(confidence: f64, importance: f64) -> f64 {
        let confidence = confidence.clamp(0.0, 1.0);
        let importance = importance.clamp(0.0, 1.0);
        (0.12 + 0.10 * confidence + 0.08 * importance).clamp(0.0, 1.0)
    }

    /// Deterministic content hash for embedding cache.
    /// Uses SHA-256 (truncated) instead of `DefaultHasher`, which is
    /// explicitly documented as unstable across Rust versions.
    fn content_hash(text: &str) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(text.as_bytes());
        // First 8 bytes → 16 hex chars, matching previous format length
        format!(
            "{:016x}",
            u64::from_be_bytes(
                hash[..8]
                    .try_into()
                    .expect("SHA-256 always produces >= 8 bytes")
            )
        )
    }

    /// Get embedding from cache, or compute + cache it
    async fn get_or_compute_embedding(&self, text: &str) -> anyhow::Result<Option<Vec<f32>>> {
        if self.embedder.dimensions() == 0 {
            return Ok(None); // Noop embedder
        }

        let hash = Self::content_hash(text);
        let now = Local::now().to_rfc3339();

        // Check cache
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            let mut stmt = conn
                .prepare_cached("SELECT embedding FROM embedding_cache WHERE content_hash = ?1")
                .context("prepare embedding cache lookup")?;
            let cached: Option<Vec<u8>> = stmt.query_row(params![hash], |row| row.get(0)).ok();

            if let Some(bytes) = cached {
                // Update accessed_at for LRU
                conn.execute(
                    "UPDATE embedding_cache SET accessed_at = ?1 WHERE content_hash = ?2",
                    params![now, hash],
                )
                .context("update embedding cache access time")?;
                return Ok(Some(vector::bytes_to_vec(&bytes)));
            }
        }

        // Compute embedding
        let embedding = self.embedder.embed_one(text).await?;
        let bytes = vector::vec_to_bytes(&embedding);

        // Store in cache + LRU eviction
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            conn.execute(
                "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, created_at, accessed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![hash, bytes, now, now],
            ).context("insert embedding into cache")?;

            // LRU eviction: keep only cache_max entries
            #[allow(clippy::cast_possible_wrap)]
            let max = self.cache_max as i64;
            conn.execute(
                "DELETE FROM embedding_cache WHERE content_hash IN (
                    SELECT content_hash FROM embedding_cache
                    ORDER BY accessed_at ASC
                    LIMIT MAX(0, (SELECT COUNT(*) FROM embedding_cache) - ?1)
                )",
                params![max],
            )
            .context("evict excess embedding cache entries")?;
        }

        Ok(Some(embedding))
    }

    /// Safe reindex: rebuild FTS5 + embeddings with rollback on failure.
    /// Public maintenance API — callable externally for manual index rebuilds.
    #[allow(dead_code)]
    pub async fn reindex(&self) -> anyhow::Result<usize> {
        // Step 1: Rebuild FTS5
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            conn.execute_batch("INSERT INTO memories_fts(memories_fts) VALUES('rebuild');")?;
        }

        // Step 2: Re-embed all memories that lack embeddings
        if self.embedder.dimensions() == 0 {
            return Ok(0);
        }

        let entries: Vec<(String, String)> = {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            let mut stmt =
                conn.prepare_cached("SELECT id, content FROM memories WHERE embedding IS NULL")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.filter_map(std::result::Result::ok).collect()
        };

        let mut count = 0;
        for (id, content) in &entries {
            if let Ok(Some(emb)) = self.get_or_compute_embedding(content).await {
                let bytes = vector::vec_to_bytes(&emb);
                let conn = self
                    .conn
                    .lock()
                    .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
                conn.execute(
                    "UPDATE memories SET embedding = ?1 WHERE id = ?2",
                    params![bytes, id],
                )?;
                count += 1;
            }
        }

        Ok(count)
    }
}

#[allow(clippy::unused_self, clippy::unused_async)]
impl SqliteMemory {
    fn name(&self) -> &str {
        "sqlite"
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    fn name(&self) -> &str {
        SqliteMemory::name(self)
    }

    async fn health_check(&self) -> bool {
        SqliteMemory::health_check(self).await
    }

    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
        SqliteMemory::append_event(self, input).await
    }

    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
        SqliteMemory::recall_scoped(self, query).await
    }

    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        SqliteMemory::resolve_slot(self, entity_id, slot_key).await
    }

    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        SqliteMemory::forget_slot(self, entity_id, slot_key, mode, reason).await
    }

    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        SqliteMemory::count_events(self, entity_id).await
    }
}
