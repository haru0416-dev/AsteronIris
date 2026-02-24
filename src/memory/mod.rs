pub mod associations;
pub mod capability;
pub mod chunker;
pub mod consolidation;
pub mod embeddings;
pub mod factory;
pub mod hygiene;
pub mod ingestion;
pub mod markdown;
pub mod sqlite;
pub mod traits;
pub mod types;
pub mod vector;

#[cfg(feature = "vector-search")]
pub mod lancedb;

pub use associations::{AssociationKind, MemoryAssociation};
pub use capability::{
    backend_capability_matrix, capability_matrix_for_backend, capability_matrix_for_memory,
    ensure_forget_mode_supported,
};
pub use consolidation::{
    CONSOLIDATION_SLOT_KEY, ConsolidationDisposition, ConsolidationInput, run_consolidation_once,
};
pub use embeddings::{
    EmbeddingProvider, NoopEmbedding, OpenAiEmbedding, create_embedding_provider,
};
pub use factory::create_memory;
pub use ingestion::{IngestionPipeline, SignalEnvelope, SqliteIngestionPipeline};
#[cfg(feature = "vector-search")]
pub use lancedb::LanceDbMemory;
pub use markdown::MarkdownMemory;
pub use sqlite::SqliteMemory;
pub use traits::Memory;
#[allow(unused_imports)]
pub use types::{
    BeliefSlot, CapabilitySupport, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, ForgetStatus, MemoryCapabilityMatrix,
    MemoryCategory, MemoryEntry, MemoryEvent, MemoryEventInput, MemoryEventType,
    MemoryInferenceEvent, MemoryLayer, MemoryProvenance, MemoryRecallItem, MemorySource,
    PrivacyLevel, RecallQuery, SignalTier, SourceKind,
};
pub use vector::{ScoredResult, cosine_similarity, hybrid_merge, rrf_merge};
