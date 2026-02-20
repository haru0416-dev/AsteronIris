pub mod chunker;
pub mod consolidation;
pub mod embeddings;
pub mod hygiene;
#[cfg(feature = "vector-search")]
pub mod lancedb;
pub mod markdown;
pub mod sqlite;
pub mod traits;
pub mod vector;

#[allow(unused_imports)]
pub use consolidation::{
    CONSOLIDATION_SLOT_KEY, ConsolidationDisposition, ConsolidationInput, ConsolidationOutput,
    enqueue_consolidation_task, run_consolidation_once,
};
#[cfg(feature = "vector-search")]
pub use lancedb::LanceDbMemory;
pub use markdown::MarkdownMemory;
pub use sqlite::SqliteMemory;
pub use traits::Memory;
#[allow(unused_imports)]
pub use traits::{
    BeliefSlot, CapabilitySupport, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, ForgetStatus, MemoryCapabilityMatrix,
    MemoryCategory, MemoryEntry, MemoryEvent, MemoryEventInput, MemoryEventType,
    MemoryInferenceEvent, MemoryProvenance, MemoryRecallItem, MemorySource, PrivacyLevel,
    RecallQuery,
};

use crate::config::MemoryConfig;
use std::path::Path;
use std::sync::Arc;

const SQLITE_CAPABILITY_MATRIX: MemoryCapabilityMatrix = MemoryCapabilityMatrix {
    backend: "sqlite",
    forget_soft: CapabilitySupport::Supported,
    forget_hard: CapabilitySupport::Supported,
    forget_tombstone: CapabilitySupport::Supported,
    unsupported_contract: "sqlite supports soft/hard/tombstone forget semantics",
};

#[cfg(feature = "vector-search")]
const LANCEDB_CAPABILITY_MATRIX: MemoryCapabilityMatrix = MemoryCapabilityMatrix {
    backend: "lancedb",
    forget_soft: CapabilitySupport::Degraded,
    forget_hard: CapabilitySupport::Supported,
    forget_tombstone: CapabilitySupport::Degraded,
    unsupported_contract: "lancedb soft/tombstone are marker rewrites; hard forget removes projection",
};

const MARKDOWN_CAPABILITY_MATRIX: MemoryCapabilityMatrix = MemoryCapabilityMatrix {
    backend: "markdown",
    forget_soft: CapabilitySupport::Degraded,
    forget_hard: CapabilitySupport::Unsupported,
    forget_tombstone: CapabilitySupport::Degraded,
    unsupported_contract: "markdown is append-only; hard forget cannot physically delete",
};

#[cfg(feature = "vector-search")]
const BACKEND_CAPABILITY_MATRIX: [MemoryCapabilityMatrix; 3] = [
    SQLITE_CAPABILITY_MATRIX,
    LANCEDB_CAPABILITY_MATRIX,
    MARKDOWN_CAPABILITY_MATRIX,
];

#[cfg(not(feature = "vector-search"))]
const BACKEND_CAPABILITY_MATRIX: [MemoryCapabilityMatrix; 2] =
    [SQLITE_CAPABILITY_MATRIX, MARKDOWN_CAPABILITY_MATRIX];

pub fn backend_capability_matrix() -> &'static [MemoryCapabilityMatrix] {
    &BACKEND_CAPABILITY_MATRIX
}

pub fn capability_matrix_for_backend(backend: &str) -> Option<MemoryCapabilityMatrix> {
    let normalized = if backend == "none" {
        "markdown"
    } else {
        backend
    };
    BACKEND_CAPABILITY_MATRIX
        .iter()
        .find(|capability| capability.backend == normalized)
        .copied()
}

#[must_use]
pub fn capability_matrix_for_memory(memory: &dyn Memory) -> MemoryCapabilityMatrix {
    capability_matrix_for_backend(memory.name()).unwrap_or(MARKDOWN_CAPABILITY_MATRIX)
}

pub fn ensure_forget_mode_supported(memory: &dyn Memory, mode: ForgetMode) -> anyhow::Result<()> {
    capability_matrix_for_memory(memory).require_forget_mode(mode)
}

/// Factory: create the right memory backend from config
pub fn create_memory(
    config: &MemoryConfig,
    workspace_dir: &Path,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Memory>> {
    // Best-effort memory hygiene/retention pass (throttled by state file).
    if let Err(e) = hygiene::run_if_due(config, workspace_dir) {
        tracing::warn!("memory hygiene skipped: {e}");
    }

    match config.backend.as_str() {
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
            Ok(Box::new(mem))
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
            Ok(Box::new(mem))
        }
        "markdown" | "none" => Ok(Box::new(MarkdownMemory::new(workspace_dir))),
        other => {
            tracing::warn!("Unknown memory backend '{other}', falling back to markdown");
            Ok(Box::new(MarkdownMemory::new(workspace_dir)))
        }
    }
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
