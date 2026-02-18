use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// "sqlite" | "lancedb" | "markdown" | "none"
    pub backend: String,
    /// Auto-save conversation context to memory
    pub auto_save: bool,
    /// Run memory/session hygiene (archiving + retention cleanup)
    #[serde(default = "default_hygiene_enabled")]
    pub hygiene_enabled: bool,
    /// Archive daily/session files older than this many days
    #[serde(default = "default_archive_after_days")]
    pub archive_after_days: u32,
    /// Purge archived files older than this many days
    #[serde(default = "default_purge_after_days")]
    pub purge_after_days: u32,
    /// For sqlite backend: prune conversation rows older than this many days
    #[serde(default = "default_conversation_retention_days")]
    pub conversation_retention_days: u32,
    #[serde(default)]
    pub layer_retention_working_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_episodic_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_semantic_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_procedural_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_identity_days: Option<u32>,
    #[serde(default)]
    pub ledger_retention_days: Option<u32>,
    /// Embedding provider: "none" | "openai" | "custom:URL"
    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,
    /// Embedding model name (e.g. "text-embedding-3-small")
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    /// Embedding vector dimensions
    #[serde(default = "default_embedding_dims")]
    pub embedding_dimensions: usize,
    /// Weight for vector similarity in hybrid search (0.0–1.0)
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f64,
    /// Weight for keyword BM25 in hybrid search (0.0–1.0)
    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f64,
    /// Max embedding cache entries before LRU eviction
    #[serde(default = "default_cache_size")]
    pub embedding_cache_size: usize,
    /// Max tokens per chunk for document splitting
    #[serde(default = "default_chunk_size")]
    pub chunk_max_tokens: usize,
}

fn default_embedding_provider() -> String {
    "none".into()
}
fn default_hygiene_enabled() -> bool {
    true
}
fn default_archive_after_days() -> u32 {
    7
}
fn default_purge_after_days() -> u32 {
    30
}
fn default_conversation_retention_days() -> u32 {
    30
}
fn default_embedding_model() -> String {
    "text-embedding-3-small".into()
}
fn default_embedding_dims() -> usize {
    1536
}
fn default_vector_weight() -> f64 {
    0.7
}
fn default_keyword_weight() -> f64 {
    0.3
}
fn default_cache_size() -> usize {
    10_000
}
fn default_chunk_size() -> usize {
    512
}

impl MemoryConfig {
    pub fn layer_retention_days(&self, layer: &str) -> u32 {
        match layer {
            "working" => self
                .layer_retention_working_days
                .unwrap_or(self.conversation_retention_days),
            "episodic" => self
                .layer_retention_episodic_days
                .unwrap_or(self.conversation_retention_days),
            "semantic" => self
                .layer_retention_semantic_days
                .unwrap_or(self.conversation_retention_days),
            "procedural" => self
                .layer_retention_procedural_days
                .unwrap_or(self.conversation_retention_days),
            "identity" => self
                .layer_retention_identity_days
                .unwrap_or(self.conversation_retention_days),
            _ => self.conversation_retention_days,
        }
    }

    pub fn ledger_retention_or_default(&self) -> u32 {
        self.ledger_retention_days
            .unwrap_or(self.conversation_retention_days)
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".into(),
            auto_save: true,
            hygiene_enabled: default_hygiene_enabled(),
            archive_after_days: default_archive_after_days(),
            purge_after_days: default_purge_after_days(),
            conversation_retention_days: default_conversation_retention_days(),
            layer_retention_working_days: None,
            layer_retention_episodic_days: None,
            layer_retention_semantic_days: None,
            layer_retention_procedural_days: None,
            layer_retention_identity_days: None,
            ledger_retention_days: None,
            embedding_provider: default_embedding_provider(),
            embedding_model: default_embedding_model(),
            embedding_dimensions: default_embedding_dims(),
            vector_weight: default_vector_weight(),
            keyword_weight: default_keyword_weight(),
            embedding_cache_size: default_cache_size(),
            chunk_max_tokens: default_chunk_size(),
        }
    }
}
