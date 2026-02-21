use async_trait::async_trait;
use std::env;
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;
use tokio::sync::Semaphore;

use asteroniris::core::memory::embeddings::EmbeddingProvider;
use asteroniris::core::memory::lancedb::LanceDbMemory;
use asteroniris::core::memory::sqlite::SqliteMemory;
use asteroniris::core::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
};

const DEFAULT_CONCURRENCY: usize = 8;
const DEFAULT_N_STORE: usize = 100;
const DEFAULT_N_RECALL: usize = 100;
const DEFAULT_RECALL_LIMIT: usize = 10;
const DEFAULT_CHURN_OPS: usize = 60;
const DEFAULT_CHURN_WINDOW: usize = 16;

const EMBEDDING_DIMS: usize = 16;
const EMBEDDING_SEED: u64 = 0x5EED_BA5E;

const PAYLOAD_BYTES: usize = 512;
const TOPICS: usize = 16;
const WORDS: usize = 64;

struct DeterministicEmbedding {
    dims: usize,
    seed: u64,
}

impl DeterministicEmbedding {
    fn with_seed(dims: usize, seed: u64) -> Self {
        Self { dims, seed }
    }

    fn fnv1a64(seed: u64, bytes: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325 ^ seed;
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    fn splitmix64(mut x: u64) -> u64 {
        x = x.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    fn u64_to_unit_f32(x: u64) -> f32 {
        const U24_MAX: f32 = ((1u32 << 24) - 1) as f32;
        let top_u24: u32 = (x >> 40) as u32;
        top_u24 as f32 / U24_MAX
    }
}

#[async_trait]
impl EmbeddingProvider for DeterministicEmbedding {
    fn name(&self) -> &str {
        "deterministic_integration"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for &t in texts {
            let base = Self::fnv1a64(self.seed, t.as_bytes());
            let mut v = Vec::with_capacity(self.dims);
            for i in 0..self.dims {
                let mixed = Self::splitmix64(base ^ (i as u64));
                v.push(Self::u64_to_unit_f32(mixed));
            }
            out.push(v);
        }
        Ok(out)
    }
}

fn fixed_payload(i: usize) -> String {
    let topic = i % TOPICS;
    let word = i % WORDS;
    let mut s = format!("topic_{topic} word_{word} key_{i:04} memory-throughput payload=");
    while s.len() < PAYLOAD_BYTES {
        s.push_str("xxxxxxxxxx");
    }
    s.truncate(PAYLOAD_BYTES);
    s
}

fn selected_backend() -> &'static str {
    let Ok(v) = env::var("BACKEND") else {
        return "sqlite";
    };

    match v.trim().to_ascii_lowercase().as_str() {
        "lancedb" => "lancedb",
        "sqlite" => "sqlite",
        _ => "sqlite",
    }
}

fn recall_query(i: usize) -> String {
    let topic = i % TOPICS;
    let word = i % WORDS;
    format!("topic_{topic} word_{word}")
}

fn env_or_default_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn ops_per_sec(ops: usize, dur: std::time::Duration) -> f64 {
    let secs = dur.as_secs_f64();
    if secs <= 0.0 {
        return f64::INFINITY;
    }
    ops as f64 / secs
}

#[tokio::test]
async fn memory_throughput_ops_per_sec() {
    let tmp = TempDir::new().unwrap();
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicEmbedding::with_seed(
        EMBEDDING_DIMS,
        EMBEDDING_SEED,
    ));

    let backend = selected_backend();
    let concurrency = env_or_default_usize("THROUGHPUT_CONCURRENCY", DEFAULT_CONCURRENCY);
    let n_store = env_or_default_usize("THROUGHPUT_N_STORE", DEFAULT_N_STORE);
    let n_recall = env_or_default_usize("THROUGHPUT_N_RECALL", DEFAULT_N_RECALL);
    let recall_limit = env_or_default_usize("THROUGHPUT_RECALL_LIMIT", DEFAULT_RECALL_LIMIT);
    println!("BACKEND={backend}");

    let mem: Arc<dyn Memory> = if backend == "lancedb" {
        Arc::new(LanceDbMemory::with_embedder(tmp.path(), Arc::clone(&embedder), 0.7, 0.3).unwrap())
    } else {
        Arc::new(
            SqliteMemory::with_embedder(tmp.path(), Arc::clone(&embedder), 0.7, 0.3, 10_000)
                .unwrap(),
        )
    };

    let store_sem = Arc::new(Semaphore::new(concurrency));
    let store_start = Instant::now();
    let mut store_set = tokio::task::JoinSet::new();

    for i in 0..n_store {
        let permit = store_sem.clone().acquire_owned().await.unwrap();
        let mem = Arc::clone(&mem);
        store_set.spawn(async move {
            let _permit = permit;
            let key = format!("k{i:04}");
            let content = fixed_payload(i);
            mem.append_event(
                MemoryEventInput::new(
                    "default",
                    key,
                    MemoryEventType::FactAdded,
                    content,
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_confidence(0.95)
                .with_importance(0.7),
            )
            .await
            .map(|_| ())
        });
    }
    while let Some(res) = store_set.join_next().await {
        res.unwrap().unwrap();
    }
    let store_dur = store_start.elapsed();
    let store_ops = ops_per_sec(n_store, store_dur);
    assert_eq!(mem.count_events(None).await.unwrap(), n_store);
    assert!(store_ops.is_finite() && store_ops > 0.0);
    let recall_sem = Arc::new(Semaphore::new(concurrency));
    let recall_start = Instant::now();
    let mut recall_set = tokio::task::JoinSet::new();

    for i in 0..n_recall {
        let permit = recall_sem.clone().acquire_owned().await.unwrap();
        let mem = Arc::clone(&mem);
        let q = recall_query(i);
        recall_set.spawn(async move {
            let _permit = permit;
            let results = mem
                .recall_scoped(RecallQuery::new("default", q, recall_limit))
                .await?;
            anyhow::ensure!(results.len() <= recall_limit);
            Ok::<(), anyhow::Error>(())
        });
    }
    while let Some(res) = recall_set.join_next().await {
        res.unwrap().unwrap();
    }
    let recall_dur = recall_start.elapsed();
    let recall_ops = ops_per_sec(n_recall, recall_dur);
    assert!(recall_ops.is_finite() && recall_ops > 0.0);
    println!("STORE_OPS_PER_SEC={store_ops:.2}");
    println!("RECALL_OPS_PER_SEC={recall_ops:.2}");
    println!("N_STORE={n_store}");
    println!("N_RECALL={n_recall}");
    println!("CONCURRENCY={concurrency}");
    println!("RECALL_LIMIT={recall_limit}");
    println!("EMBEDDING_DIMS={EMBEDDING_DIMS}");
    println!("PAYLOAD_BYTES={PAYLOAD_BYTES}");
}

#[tokio::test]
async fn memory_throughput_churn_bounded_recall() {
    let tmp = TempDir::new().unwrap();
    let embedder: Arc<dyn EmbeddingProvider> = Arc::new(DeterministicEmbedding::with_seed(
        EMBEDDING_DIMS,
        EMBEDDING_SEED,
    ));

    let backend = selected_backend();
    let concurrency = env_or_default_usize("THROUGHPUT_CONCURRENCY", DEFAULT_CONCURRENCY);
    let n_ops = env_or_default_usize("THROUGHPUT_CHURN_OPS", DEFAULT_CHURN_OPS);
    let churn_window = env_or_default_usize("THROUGHPUT_CHURN_WINDOW", DEFAULT_CHURN_WINDOW);
    let recall_limit = env_or_default_usize("THROUGHPUT_RECALL_LIMIT", DEFAULT_RECALL_LIMIT);
    println!("CHURN_BACKEND={backend}");

    let mem: Arc<dyn Memory> = if backend == "lancedb" {
        Arc::new(LanceDbMemory::with_embedder(tmp.path(), Arc::clone(&embedder), 0.7, 0.3).unwrap())
    } else {
        Arc::new(
            SqliteMemory::with_embedder(tmp.path(), Arc::clone(&embedder), 0.7, 0.3, 10_000)
                .unwrap(),
        )
    };

    let churn_sem = Arc::new(Semaphore::new(concurrency));
    let churn_start = Instant::now();
    let mut churn_set = tokio::task::JoinSet::new();

    for i in 0..n_ops {
        let permit = churn_sem.clone().acquire_owned().await.unwrap();
        let mem = Arc::clone(&mem);
        let slot_key = format!("churn.k{:04}", i % churn_window.max(1));
        let content = fixed_payload(i);
        let query = recall_query(i);
        churn_set.spawn(async move {
            let _permit = permit;

            mem.append_event(
                MemoryEventInput::new(
                    "default",
                    slot_key,
                    MemoryEventType::FactAdded,
                    content,
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_confidence(0.95)
                .with_importance(0.7),
            )
            .await?;

            let results = mem
                .recall_scoped(RecallQuery::new("default", query, recall_limit))
                .await?;
            anyhow::ensure!(results.len() <= recall_limit);
            anyhow::ensure!(results.iter().all(|item| item.score.is_finite()));

            Ok::<(), anyhow::Error>(())
        });
    }

    while let Some(res) = churn_set.join_next().await {
        res.unwrap().unwrap();
    }

    let churn_dur = churn_start.elapsed();
    let churn_ops = ops_per_sec(n_ops, churn_dur);
    assert!(churn_ops.is_finite() && churn_ops > 0.0);

    let mut matched_queries = 0usize;
    for topic in 0..TOPICS {
        let query = format!("topic_{topic}");
        let results = mem
            .recall_scoped(RecallQuery::new("default", query.clone(), recall_limit))
            .await
            .unwrap();
        assert!(results.len() <= recall_limit);
        if results.iter().any(|item| item.value.contains(&query)) {
            matched_queries += 1;
        }
    }

    assert!(
        matched_queries >= TOPICS / 2,
        "retrieval quality under churn regressed (matched_queries={matched_queries})"
    );
    assert_eq!(mem.count_events(None).await.unwrap(), n_ops);

    println!("CHURN_OPS_PER_SEC={churn_ops:.2}");
    println!("CHURN_N_OPS={n_ops}");
    println!("CHURN_WINDOW={churn_window}");
    println!("CHURN_MATCHED_QUERIES={matched_queries}");
    println!("CHURN_RECALL_LIMIT={recall_limit}");
}
