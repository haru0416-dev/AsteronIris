mod codec;
mod events;
mod projection;
mod repository;
mod schema;
mod search;

use crate::memory::associations::MemoryAssociation;
use crate::memory::embeddings::EmbeddingProvider;
use crate::memory::traits::Memory;
use crate::memory::types::{
    BeliefSlot, ForgetMode, ForgetOutcome, MemoryEvent, MemoryEventInput, MemoryRecallItem,
    RecallQuery,
};
use anyhow::Context;
use sqlx::SqlitePool;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

/// SQLite-backed persistent memory.
///
/// Full-stack search engine:
/// - **Vector DB**: embeddings stored as BLOB, cosine-similarity search
/// - **Keyword Search**: FTS5 virtual table with BM25 scoring
/// - **Hybrid Merge**: RRF fusion of vector + keyword results
/// - **Embedding Cache**: LRU-evicted cache to avoid redundant API calls
pub struct SqliteMemory {
    pool: SqlitePool,
    embedder: Arc<dyn EmbeddingProvider>,
    cache_max: usize,
}

impl SqliteMemory {
    /// Open (or create) the database at `<workspace_dir>/memory/brain.db`.
    pub async fn new(workspace_dir: &Path) -> anyhow::Result<Self> {
        Self::with_embedder(
            workspace_dir,
            Arc::new(crate::memory::embeddings::NoopEmbedding),
            10_000,
        )
        .await
    }

    /// Open with a custom embedding provider and cache size.
    pub async fn with_embedder(
        workspace_dir: &Path,
        embedder: Arc<dyn EmbeddingProvider>,
        cache_max: usize,
    ) -> anyhow::Result<Self> {
        let db_path = workspace_dir.join("memory").join("brain.db");

        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("create memory directory")?;
        }

        let url = format!("sqlite:{}?mode=rwc", db_path.display());
        let pool = SqlitePool::connect(&url)
            .await
            .context("open SQLite database")?;

        schema::init_schema(&pool).await?;

        Ok(Self {
            pool,
            embedder,
            cache_max,
        })
    }

    /// Open an in-memory database (useful for tests).
    #[cfg(test)]
    pub async fn in_memory() -> anyhow::Result<Self> {
        Self::in_memory_with_embedder(Arc::new(crate::memory::embeddings::NoopEmbedding), 10_000)
            .await
    }

    /// Open an in-memory database with a custom embedder.
    #[cfg(test)]
    pub async fn in_memory_with_embedder(
        embedder: Arc<dyn EmbeddingProvider>,
        cache_max: usize,
    ) -> anyhow::Result<Self> {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .context("open in-memory SQLite")?;
        schema::init_schema(&pool).await?;
        Ok(Self {
            pool,
            embedder,
            cache_max,
        })
    }

    /// Rebuild FTS5 index and re-embed entries missing embeddings.
    pub async fn reindex(&self) -> anyhow::Result<usize> {
        sqlx::raw_sql("INSERT INTO retrieval_fts(retrieval_fts) VALUES('rebuild');")
            .execute(&self.pool)
            .await
            .context("rebuild FTS5 index")?;

        if self.embedder.dimensions() == 0 {
            return Ok(0);
        }

        let entries: Vec<(String, String)> =
            sqlx::query_as("SELECT unit_id, content FROM retrieval_units WHERE embedding IS NULL")
                .fetch_all(&self.pool)
                .await
                .context("fetch entries for reindex")?;

        let mut count = 0;
        for (id, content) in &entries {
            if let Ok(emb) = self.embedder.embed_one(content).await {
                let bytes = crate::memory::vector::vec_to_bytes(&emb);
                sqlx::query("UPDATE retrieval_units SET embedding = ?1 WHERE unit_id = ?2")
                    .bind(&bytes)
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .context("update embedding during reindex")?;
                count += 1;
            }
        }

        Ok(count)
    }
}

impl Memory for SqliteMemory {
    fn name(&self) -> &str {
        "sqlite"
    }

    fn health_check(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move { repository::health_check(&self.pool).await })
    }

    fn append_event(
        &self,
        input: MemoryEventInput,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MemoryEvent>> + Send + '_>> {
        Box::pin(async move {
            repository::append_event(&self.pool, &self.embedder, self.cache_max, input).await
        })
    }

    fn recall_scoped(
        &self,
        query: RecallQuery,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryRecallItem>>> + Send + '_>> {
        Box::pin(async move {
            repository::recall_scoped(&self.pool, &self.embedder, self.cache_max, query).await
        })
    }

    fn resolve_slot<'a>(
        &'a self,
        entity_id: &'a str,
        slot_key: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<BeliefSlot>>> + Send + 'a>> {
        Box::pin(async move { repository::resolve_slot(&self.pool, entity_id, slot_key).await })
    }

    fn forget_slot<'a>(
        &'a self,
        entity_id: &'a str,
        slot_key: &'a str,
        mode: ForgetMode,
        reason: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ForgetOutcome>> + Send + 'a>> {
        Box::pin(async move {
            repository::forget_slot(&self.pool, entity_id, slot_key, mode, reason).await
        })
    }

    fn count_events<'a>(
        &'a self,
        entity_id: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<usize>> + Send + 'a>> {
        Box::pin(async move { repository::count_events(&self.pool, entity_id).await })
    }

    fn add_association<'a>(
        &'a self,
        association: MemoryAssociation,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move { repository::add_association(&self.pool, association).await })
    }

    fn get_associations<'a>(
        &'a self,
        entry_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryAssociation>>> + Send + 'a>> {
        Box::pin(async move { repository::get_associations(&self.pool, entry_id).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::associations::AssociationKind;
    use crate::memory::types::{MemoryEventType, MemorySource, PrivacyLevel};

    #[tokio::test]
    async fn health_check_passes() {
        let mem = SqliteMemory::in_memory().await.unwrap();
        assert!(mem.health_check().await);
    }

    #[tokio::test]
    async fn append_and_resolve_slot() {
        let mem = SqliteMemory::in_memory().await.unwrap();

        let input = MemoryEventInput::new(
            "entity:test",
            "profile.name",
            MemoryEventType::FactAdded,
            "Alice",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );
        let event = mem.append_event(input).await.unwrap();
        assert!(!event.event_id.is_empty());

        let slot = mem
            .resolve_slot("entity:test", "profile.name")
            .await
            .unwrap();
        assert!(slot.is_some());
        let slot = slot.unwrap();
        assert_eq!(slot.value, "Alice");
    }

    #[tokio::test]
    async fn forget_soft_hides_slot() {
        let mem = SqliteMemory::in_memory().await.unwrap();

        let input = MemoryEventInput::new(
            "entity:test",
            "profile.email",
            MemoryEventType::FactAdded,
            "alice@example.com",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );
        mem.append_event(input).await.unwrap();

        let outcome = mem
            .forget_slot("entity:test", "profile.email", ForgetMode::Soft, "GDPR")
            .await
            .unwrap();
        assert!(outcome.applied);

        // Slot should no longer be active
        let slot = mem
            .resolve_slot("entity:test", "profile.email")
            .await
            .unwrap();
        assert!(slot.is_none());
    }

    #[tokio::test]
    async fn count_events_works() {
        let mem = SqliteMemory::in_memory().await.unwrap();

        let input = MemoryEventInput::new(
            "entity:test",
            "profile.age",
            MemoryEventType::FactAdded,
            "30",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );
        mem.append_event(input).await.unwrap();

        let count = mem.count_events(Some("entity:test")).await.unwrap();
        assert_eq!(count, 1);

        let count_all = mem.count_events(None).await.unwrap();
        assert_eq!(count_all, 1);
    }

    #[tokio::test]
    async fn add_and_get_associations() {
        let mem = SqliteMemory::in_memory().await.unwrap();

        let assoc = MemoryAssociation::new("entry:a", "entry:b", AssociationKind::RelatedTo);
        mem.add_association(assoc).await.unwrap();

        let results = mem.get_associations("entry:a").await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_id, "entry:a");
        assert_eq!(results[0].target_id, "entry:b");
        assert_eq!(results[0].kind, AssociationKind::RelatedTo);

        // Also retrievable via target_id
        let results = mem.get_associations("entry:b").await.unwrap();
        assert_eq!(results.len(), 1);
    }
}
