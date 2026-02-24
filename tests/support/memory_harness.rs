#![allow(dead_code, clippy::needless_lifetimes, clippy::cast_precision_loss)]

use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use tempfile::TempDir;

use asteroniris::memory::embeddings::EmbeddingProvider;
use asteroniris::memory::{
    CapabilitySupport, ForgetMode, LanceDbMemory, MarkdownMemory, Memory, MemoryCapabilityMatrix,
    MemoryCategory, MemoryEventInput, MemoryEventType, MemoryRecallItem, MemorySource,
    PrivacyLevel, RecallQuery, SqliteMemory, backend_capability_matrix,
};

pub const LANCEDB_EMBEDDING_DIMS: usize = 8;
pub const LANCEDB_EMBEDDING_SEED: u64 = 0x5EED_BA5E;
pub const LANCEDB_VECTOR_WEIGHT: f32 = 0.7;
pub const LANCEDB_KEYWORD_WEIGHT: f32 = 0.3;

struct DeterministicEmbeddingProvider {
    dims: usize,
    seed: u64,
}

impl DeterministicEmbeddingProvider {
    const fn new(dims: usize, seed: u64) -> Self {
        Self { dims, seed }
    }

    fn fnv1a64(seed: u64, bytes: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    fn splitmix64(mut x: u64) -> u64 {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    #[allow(clippy::cast_precision_loss)]
    fn unit_f32(x: u64) -> f32 {
        const U24_MAX: f32 = ((1u32 << 24) - 1) as f32;
        let top_u24: u32 = (x >> 40) as u32;
        (top_u24 as f32 / U24_MAX) * 2.0 - 1.0
    }

    fn embed_value(&self, text: &str, index: usize) -> f32 {
        let base = Self::fnv1a64(self.seed, text.as_bytes());
        let mixed = Self::splitmix64(base ^ (index as u64));
        Self::unit_f32(mixed)
    }
}

impl EmbeddingProvider for DeterministicEmbeddingProvider {
    fn name(&self) -> &str {
        "memory-test-harness"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    fn embed<'a>(
        &'a self,
        texts: &'a [&'a str],
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<Vec<f32>>>> + Send + 'a>> {
        Box::pin(async move {
            let mut vectors = Vec::with_capacity(texts.len());
            for text in texts {
                let mut vector = Vec::with_capacity(self.dims);
                for idx in 0..self.dims {
                    vector.push(self.embed_value(text, idx));
                }
                vectors.push(vector);
            }
            Ok(vectors)
        })
    }
}

pub async fn sqlite_memory_from_path(path: &Path) -> SqliteMemory {
    SqliteMemory::new(path)
        .await
        .expect("sqlite memory backend should initialize")
}

pub fn markdown_memory_from_path(path: &Path) -> MarkdownMemory {
    MarkdownMemory::new(path)
}

pub fn lancedb_memory_from_path(path: &Path) -> LanceDbMemory {
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicEmbeddingProvider::new(
        LANCEDB_EMBEDDING_DIMS,
        LANCEDB_EMBEDDING_SEED,
    ));
    LanceDbMemory::with_embedder(
        path,
        embedder,
        LANCEDB_VECTOR_WEIGHT,
        LANCEDB_KEYWORD_WEIGHT,
    )
    .expect("lancedb memory backend should initialize with deterministic embedder")
}

pub async fn sqlite_fixture() -> (TempDir, SqliteMemory) {
    let temp_dir = TempDir::new().expect("temp directory should be created");
    let memory = sqlite_memory_from_path(temp_dir.path()).await;
    (temp_dir, memory)
}

pub fn markdown_fixture() -> (TempDir, MarkdownMemory) {
    let temp_dir = TempDir::new().expect("temp directory should be created");
    let memory = markdown_memory_from_path(temp_dir.path());
    (temp_dir, memory)
}

pub fn lancedb_fixture() -> (TempDir, LanceDbMemory) {
    let temp_dir = TempDir::new().expect("temp directory should be created");
    let memory = lancedb_memory_from_path(temp_dir.path());
    (temp_dir, memory)
}

pub fn source_for_category(category: &MemoryCategory) -> MemorySource {
    match category {
        MemoryCategory::Core => MemorySource::ExplicitUser,
        MemoryCategory::Daily => MemorySource::System,
        MemoryCategory::Conversation => MemorySource::Inferred,
        MemoryCategory::Custom(_) => MemorySource::ToolVerified,
    }
}

pub async fn append_test_event(
    memory: &dyn Memory,
    entity_id: &str,
    slot_key: &str,
    value: &str,
    category: MemoryCategory,
) {
    let source = source_for_category(&category);
    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                slot_key,
                MemoryEventType::FactAdded,
                value,
                source,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.6),
        )
        .await
        .expect("test event append should succeed");
}

pub async fn memory_count(memory: &dyn Memory) -> usize {
    memory
        .count_events(None)
        .await
        .expect("count_events should succeed")
}

pub async fn resolve_slot_value(
    memory: &dyn Memory,
    entity_id: &str,
    slot_key: &str,
) -> Option<String> {
    let resolved = memory
        .resolve_slot(entity_id, slot_key)
        .await
        .expect("resolve_slot should succeed")
        .map(|slot| slot.value);

    resolved.map(|value| normalize_slot_value(&value).to_string())
}

fn normalize_slot_value(value: &str) -> &str {
    value
        .strip_prefix("**")
        .and_then(|without_prefix| {
            without_prefix
                .split_once("**: ")
                .map(|(_, payload)| payload)
        })
        .unwrap_or(value)
}

pub async fn recall_scoped_values(
    memory: &dyn Memory,
    entity_id: &str,
    query: &str,
    limit: usize,
) -> Vec<(String, String, f64)> {
    let items = recall_scoped_items(memory, entity_id, query, limit).await;
    items
        .into_iter()
        .map(|item| (item.slot_key, item.value, item.score))
        .collect()
}

pub async fn recall_scoped_items(
    memory: &dyn Memory,
    entity_id: &str,
    query: &str,
    limit: usize,
) -> Vec<MemoryRecallItem> {
    memory
        .recall_scoped(RecallQuery::new(entity_id, query, limit))
        .await
        .expect("recall_scoped should succeed")
}

pub async fn forget_hard(memory: &dyn Memory, entity_id: &str, slot_key: &str) -> bool {
    memory
        .forget_slot(entity_id, slot_key, ForgetMode::Hard, "test")
        .await
        .expect("forget_slot should run")
        .applied
}

#[derive(Debug)]
pub enum ParityRelation {
    Exact,
    AtLeast,
}

pub fn assert_event_count_parity(relation: ParityRelation, lhs: usize, rhs: usize, message: &str) {
    match relation {
        ParityRelation::Exact => {
            assert_eq!(lhs, rhs, "{} (lhs={lhs}, rhs={rhs})", message);
        }
        ParityRelation::AtLeast => {
            assert!(lhs >= rhs, "{} (lhs={lhs}, rhs={rhs})", message);
        }
    }
}

pub fn format_capability_evidence() -> String {
    let mut lines = Vec::new();
    for matrix in backend_capability_matrix() {
        lines.push(format_capability_row(matrix));
    }
    lines.join("\n")
}

fn format_capability_row(matrix: &MemoryCapabilityMatrix) -> String {
    let soft = format_support(matrix.forget_soft);
    let hard = format_support(matrix.forget_hard);
    let tombstone = format_support(matrix.forget_tombstone);
    format!(
        "backend={} soft={} hard={} tombstone={} contract={}",
        matrix.backend, soft, hard, tombstone, matrix.unsupported_contract
    )
}

fn format_support(support: CapabilitySupport) -> &'static str {
    match support {
        CapabilitySupport::Supported => "SUPPORTED",
        CapabilitySupport::Degraded => "DEGRADED",
        CapabilitySupport::Unsupported => "UNSUPPORTED",
    }
}

pub fn find_degraded_backends() -> Vec<&'static str> {
    backend_capability_matrix()
        .iter()
        .filter(|entry| {
            entry.forget_soft == CapabilitySupport::Degraded
                || entry.forget_hard == CapabilitySupport::Degraded
                || entry.forget_tombstone == CapabilitySupport::Degraded
                || entry.forget_tombstone == CapabilitySupport::Unsupported
                || entry.forget_hard == CapabilitySupport::Unsupported
                || entry.forget_soft == CapabilitySupport::Unsupported
        })
        .map(|entry| entry.backend)
        .collect()
}

pub fn capture_recall_items_as_csv(items: &[MemoryRecallItem]) -> String {
    let mut out = String::new();
    for item in items {
        use fmt::Write as _;
        writeln!(
            &mut out,
            "{},{},{:.6}",
            item.slot_key, item.entity_id, item.score
        )
        .expect("string building should not fail");
    }
    out
}
