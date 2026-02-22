use crate::config::MemoryConfig;

#[cfg(feature = "vector-search")]
use super::LanceDbMemory;
use super::{
    MarkdownMemory, Memory, MemoryEvent, MemoryInferenceEvent, SqliteMemory, embeddings, hygiene,
};

use std::path::Path;
use std::sync::Arc;

pub fn create_memory(
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

            #[allow(clippy::cast_possible_truncation)]
            let mem = SqliteMemory::with_embedder(
                workspace_dir,
                embedder,
                config.vector_weight as f32,
                config.keyword_weight as f32,
                config.embedding_cache_size,
            )?;
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
            tracing::warn!("Unknown memory backend '{other}', falling back to markdown");
            Box::new(MarkdownMemory::new(workspace_dir))
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

    use std::fs;

    use tempfile::TempDir;

    #[test]
    fn factory_sqlite() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "sqlite".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).unwrap();
        assert_eq!(mem.name(), "sqlite");
    }

    #[test]
    fn factory_markdown() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).unwrap();
        assert_eq!(mem.name(), "markdown");
    }

    #[test]
    fn factory_none_falls_back_to_markdown() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "none".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).unwrap();
        assert_eq!(mem.name(), "markdown");
    }

    #[test]
    fn factory_unknown_falls_back_to_markdown() {
        let tmp = TempDir::new().unwrap();
        let cfg = MemoryConfig {
            backend: "redis".into(),
            ..MemoryConfig::default()
        };
        let mem = create_memory(&cfg, tmp.path(), None).unwrap();
        assert_eq!(mem.name(), "markdown");
    }

    #[test]
    fn memory_hygiene_failure_nonfatal() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("state"), "not-json").unwrap();

        let cfg = MemoryConfig {
            backend: "sqlite".into(),
            ..MemoryConfig::default()
        };

        let mem = create_memory(&cfg, tmp.path(), None).unwrap();
        assert_eq!(mem.name(), "sqlite");
    }
}
