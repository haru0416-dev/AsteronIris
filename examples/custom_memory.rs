//! Example: Implementing a custom Memory backend for AsteronIris
//!
//! Demonstrates the dyn-safe async trait pattern. The Memory trait uses
//! `Pin<Box<dyn Future<Output = R> + Send + '_>>` so backends can be stored
//! as `Arc<dyn Memory>` and shared across async tasks.
//!
//! Run: `cargo run --example custom_memory`

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};

// ── Minimal types (mirrors src/core/memory/traits.rs) ───────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    ExplicitUser,
    Inferred,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub key: String,
    pub value: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub occurred_at: String,
}

// ── Minimal Memory trait ────────────────────────────────────────────

pub trait Memory: Send + Sync {
    fn name(&self) -> &str;

    fn store<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
        source: MemorySource,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    fn recall<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryEntry>>> + Send + 'a>>;

    fn forget<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;

    fn count(&self) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + '_>>;
}

// ── In-memory HashMap backend ───────────────────────────────────────

pub struct InMemoryBackend {
    store: Mutex<HashMap<String, MemoryEntry>>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl Memory for InMemoryBackend {
    fn name(&self) -> &str {
        "in-memory"
    }

    fn store<'a>(
        &'a self,
        key: &'a str,
        value: &'a str,
        source: MemorySource,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let entry = MemoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                key: key.to_string(),
                value: value.to_string(),
                source,
                confidence: 1.0,
                occurred_at: chrono::Local::now().to_rfc3339(),
            };
            self.store
                .lock()
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .insert(key.to_string(), entry);
            Ok(())
        })
    }

    fn recall<'a>(
        &'a self,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<MemoryEntry>>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.store.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            let query_lower = query.to_lowercase();
            let mut results: Vec<MemoryEntry> = store
                .values()
                .filter(|e| e.value.to_lowercase().contains(&query_lower))
                .cloned()
                .collect();
            results.truncate(limit);
            Ok(results)
        })
    }

    fn forget<'a>(
        &'a self,
        key: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            let mut store = self.store.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(store.remove(key).is_some())
        })
    }

    fn count(&self) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + '_>> {
        Box::pin(async move {
            let store = self.store.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(store.len())
        })
    }
}

// ── Demo ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Memory backends are stored as `Arc<dyn Memory>` — the trait is dyn-safe.
    let brain: Box<dyn Memory> = Box::new(InMemoryBackend::new());

    println!("AsteronIris Memory Demo — {}\n", brain.name());

    brain
        .store("user_lang", "User prefers Rust", MemorySource::ExplicitUser)
        .await?;
    brain
        .store("user_tz", "Timezone is EST", MemorySource::ExplicitUser)
        .await?;
    brain
        .store(
            "today_note",
            "Completed memory system implementation",
            MemorySource::System,
        )
        .await?;

    println!("Stored {} memories", brain.count().await?);

    let results = brain.recall("Rust", 5).await?;
    println!("\nRecall 'Rust' -> {} results:", results.len());
    for entry in &results {
        println!("  [{:?}] {}: {}", entry.source, entry.key, entry.value);
    }

    let removed = brain.forget("user_tz").await?;
    println!("\nForget 'user_tz' -> removed: {removed}");
    println!("Remaining: {} memories", brain.count().await?);

    println!("\nMemory backend works! Implement the Memory trait for any storage.");
    Ok(())
}
