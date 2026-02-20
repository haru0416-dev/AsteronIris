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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::traits::MemoryLayer;

    #[test]
    fn default_memory_config_values() {
        let config = MemoryConfig::default();

        assert_eq!(config.backend, "sqlite");
        assert!(config.auto_save);
        assert!(config.hygiene_enabled);
        assert_eq!(config.archive_after_days, 7);
        assert_eq!(config.purge_after_days, 30);
        assert_eq!(config.conversation_retention_days, 30);
        assert_eq!(config.layer_retention_working_days, None);
        assert_eq!(config.layer_retention_episodic_days, None);
        assert_eq!(config.layer_retention_semantic_days, None);
        assert_eq!(config.layer_retention_procedural_days, None);
        assert_eq!(config.layer_retention_identity_days, None);
        assert_eq!(config.ledger_retention_days, None);
        assert_eq!(config.embedding_provider, "none");
        assert_eq!(config.embedding_model, "text-embedding-3-small");
        assert_eq!(config.embedding_dimensions, 1536);
        assert_eq!(config.vector_weight, 0.7);
        assert_eq!(config.keyword_weight, 0.3);
        assert_eq!(config.embedding_cache_size, 10_000);
        assert_eq!(config.chunk_max_tokens, 512);
    }

    #[test]
    fn layer_retention_days_uses_layer_specific_values() {
        let config = MemoryConfig {
            conversation_retention_days: 30,
            layer_retention_working_days: Some(3),
            layer_retention_episodic_days: Some(14),
            layer_retention_semantic_days: Some(90),
            layer_retention_procedural_days: Some(120),
            layer_retention_identity_days: Some(365),
            ..MemoryConfig::default()
        };

        let cases = [
            (MemoryLayer::Working, "working", 3),
            (MemoryLayer::Episodic, "episodic", 14),
            (MemoryLayer::Semantic, "semantic", 90),
            (MemoryLayer::Procedural, "procedural", 120),
            (MemoryLayer::Identity, "identity", 365),
        ];

        for (_layer, layer_name, expected_days) in cases {
            assert_eq!(config.layer_retention_days(layer_name), expected_days);
        }
    }

    #[test]
    fn layer_retention_days_falls_back_to_conversation_retention() {
        let config = MemoryConfig {
            conversation_retention_days: 45,
            ..MemoryConfig::default()
        };

        assert_eq!(config.layer_retention_days("working"), 45);
        assert_eq!(config.layer_retention_days("episodic"), 45);
        assert_eq!(config.layer_retention_days("semantic"), 45);
        assert_eq!(config.layer_retention_days("procedural"), 45);
        assert_eq!(config.layer_retention_days("identity"), 45);
        assert_eq!(config.layer_retention_days("unknown"), 45);
    }

    #[test]
    fn ledger_retention_or_default_respects_override() {
        let with_override = MemoryConfig {
            conversation_retention_days: 30,
            ledger_retention_days: Some(180),
            ..MemoryConfig::default()
        };
        assert_eq!(with_override.ledger_retention_or_default(), 180);

        let without_override = MemoryConfig {
            conversation_retention_days: 60,
            ledger_retention_days: None,
            ..MemoryConfig::default()
        };
        assert_eq!(without_override.ledger_retention_or_default(), 60);
    }

    #[test]
    fn memory_config_toml_round_trip() {
        let original = MemoryConfig {
            backend: "markdown".into(),
            auto_save: false,
            hygiene_enabled: false,
            archive_after_days: 3,
            purge_after_days: 12,
            conversation_retention_days: 48,
            layer_retention_working_days: Some(2),
            layer_retention_episodic_days: Some(10),
            layer_retention_semantic_days: Some(60),
            layer_retention_procedural_days: Some(120),
            layer_retention_identity_days: Some(365),
            ledger_retention_days: Some(90),
            embedding_provider: "custom:https://embed.example".into(),
            embedding_model: "example-embed-v1".into(),
            embedding_dimensions: 1024,
            vector_weight: 0.65,
            keyword_weight: 0.35,
            embedding_cache_size: 2048,
            chunk_max_tokens: 256,
        };

        let toml = toml::to_string(&original).unwrap();
        let decoded: MemoryConfig = toml::from_str(&toml).unwrap();

        assert_eq!(decoded.backend, original.backend);
        assert_eq!(decoded.auto_save, original.auto_save);
        assert_eq!(decoded.hygiene_enabled, original.hygiene_enabled);
        assert_eq!(decoded.archive_after_days, original.archive_after_days);
        assert_eq!(decoded.purge_after_days, original.purge_after_days);
        assert_eq!(
            decoded.conversation_retention_days,
            original.conversation_retention_days
        );
        assert_eq!(
            decoded.layer_retention_working_days,
            original.layer_retention_working_days
        );
        assert_eq!(
            decoded.layer_retention_episodic_days,
            original.layer_retention_episodic_days
        );
        assert_eq!(
            decoded.layer_retention_semantic_days,
            original.layer_retention_semantic_days
        );
        assert_eq!(
            decoded.layer_retention_procedural_days,
            original.layer_retention_procedural_days
        );
        assert_eq!(
            decoded.layer_retention_identity_days,
            original.layer_retention_identity_days
        );
        assert_eq!(
            decoded.ledger_retention_days,
            original.ledger_retention_days
        );
        assert_eq!(decoded.embedding_provider, original.embedding_provider);
        assert_eq!(decoded.embedding_model, original.embedding_model);
        assert_eq!(decoded.embedding_dimensions, original.embedding_dimensions);
        assert_eq!(decoded.vector_weight, original.vector_weight);
        assert_eq!(decoded.keyword_weight, original.keyword_weight);
        assert_eq!(decoded.embedding_cache_size, original.embedding_cache_size);
        assert_eq!(decoded.chunk_max_tokens, original.chunk_max_tokens);
    }
}
