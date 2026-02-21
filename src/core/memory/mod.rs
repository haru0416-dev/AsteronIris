mod capability;
pub mod chunker;
pub mod consolidation;
pub mod embeddings;
mod factory;
pub mod hygiene;
#[cfg(feature = "vector-search")]
pub mod lancedb;
pub mod markdown;
pub mod memory_types;
pub mod sqlite;
pub mod traits;
pub mod vector;

#[allow(unused_imports)]
pub use capability::{
    backend_capability_matrix, capability_matrix_for_backend, capability_matrix_for_memory,
    ensure_forget_mode_supported,
};
#[allow(unused_imports)]
pub use consolidation::{
    CONSOLIDATION_SLOT_KEY, ConsolidationDisposition, ConsolidationInput, ConsolidationOutput,
    enqueue_consolidation_task, run_consolidation_once,
};
#[allow(unused_imports)]
pub use factory::{create_memory, persist_inference_events};
#[cfg(feature = "vector-search")]
pub use lancedb::LanceDbMemory;
pub use markdown::MarkdownMemory;
#[allow(unused_imports)]
pub use memory_types::{
    BeliefSlot, CapabilitySupport, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, ForgetStatus, MemoryCapabilityMatrix,
    MemoryCategory, MemoryEntry, MemoryEvent, MemoryEventInput, MemoryEventType,
    MemoryInferenceEvent, MemoryProvenance, MemoryRecallItem, MemorySource, PrivacyLevel,
    RecallQuery,
};
pub use sqlite::SqliteMemory;
pub use traits::Memory;
