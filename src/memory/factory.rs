use crate::config::MemoryConfig;

#[cfg(feature = "vector-search")]
use super::lancedb::LanceDbMemory;
use super::{
    Memory, MemoryEvent, MemoryInferenceEvent, SqliteMemory, embeddings, hygiene,
    markdown::MarkdownMemory,
};

use std::path::Path;
use std::sync::Arc;

pub async fn create_memory(
    config: &MemoryConfig,
    workspace_dir: &Path,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Memory>> {
    let memory: Box<dyn Memory> = match config.backend.as_str() {
        "sqlite" => {
            let embedder: Arc<dyn embeddings::EmbeddingProvider> =
                Arc::from(embeddings::create_embedding_provider(
                    &config.embedding_provider,
                    api_key,
                    &config.embedding_model,
                    config.embedding_dimensions,
                ));

            let mem =
                SqliteMemory::with_embedder(workspace_dir, embedder, config.embedding_cache_size)
                    .await?;
            Box::new(mem)
        }
        #[cfg(feature = "vector-search")]
        "lancedb" => {
            let embedder: Arc<dyn embeddings::EmbeddingProvider> =
                Arc::from(embeddings::create_embedding_provider(
                    &config.embedding_provider,
                    api_key,
                    &config.embedding_model,
                    config.embedding_dimensions,
                ));

            #[allow(clippy::cast_possible_truncation)]
            let mem = LanceDbMemory::with_embedder(
                workspace_dir,
                embedder,
                config.vector_weight as f32,
                config.keyword_weight as f32,
            )?;
            Box::new(mem)
        }
        "markdown" | "none" => Box::new(MarkdownMemory::new(workspace_dir)),
        other => {
            anyhow::bail!(
                "Unknown memory backend '{other}'. Supported: sqlite, lancedb, markdown, none"
            );
        }
    };

    if let Err(e) = hygiene::run_if_due(config, workspace_dir) {
        tracing::warn!("memory hygiene skipped: {e}");
    }

    Ok(memory)
}

pub async fn persist_inference_events(
    memory: &dyn Memory,
    events: Vec<MemoryInferenceEvent>,
) -> anyhow::Result<Vec<MemoryEvent>> {
    memory.append_inference_events(events).await
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[tokio::test]
    async fn factory_sqlite() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "sqlite".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).await.unwrap();
        assert_eq!(mem.name(), "sqlite");
    }

    #[tokio::test]
    async fn factory_markdown() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).await.unwrap();
        assert_eq!(mem.name(), "markdown");
    }

    #[tokio::test]
    async fn factory_none_falls_back_to_markdown() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "none".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).await.unwrap();
        assert_eq!(mem.name(), "markdown");
    }

    #[tokio::test]
    async fn factory_unknown_backend_is_error() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "redis".into(),
            ..MemoryConfig::default()
        };
        match create_memory(&cfg, tmp.path(), None).await {
            Err(e) => assert!(
                e.to_string().contains("Unknown memory backend"),
                "expected unknown backend error, got: {e}"
            ),
            Ok(_) => panic!("expected error for unknown backend"),
        }
    }

    #[tokio::test]
    async fn memory_hygiene_failure_nonfatal() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("state"), "not-json").unwrap();

        let cfg = MemoryConfig {
            backend: "sqlite".into(),
            ..MemoryConfig::default()
        };

        let mem = create_memory(&cfg, tmp.path(), None).await.unwrap();
        assert_eq!(mem.name(), "sqlite");
    }
}
