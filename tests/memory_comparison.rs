//! Head-to-head comparison: SQLite vs Markdown memory backends
//!
//! Run with: cargo test --test memory_comparison -- --nocapture

use std::time::Instant;
use tempfile::TempDir;

// We test both backends through the public memory module
use asteroniris::memory::{
    markdown::MarkdownMemory, sqlite::SqliteMemory, ForgetMode, Memory, MemoryCategory,
    MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
};

// ── Helpers ────────────────────────────────────────────────────

fn sqlite_backend(dir: &std::path::Path) -> SqliteMemory {
    SqliteMemory::new(dir).expect("SQLite init failed")
}

fn markdown_backend(dir: &std::path::Path) -> MarkdownMemory {
    MarkdownMemory::new(dir)
}

fn source_for_category(category: &MemoryCategory) -> MemorySource {
    match category {
        MemoryCategory::Core => MemorySource::ExplicitUser,
        MemoryCategory::Daily => MemorySource::System,
        MemoryCategory::Conversation => MemorySource::Inferred,
        MemoryCategory::Custom(_) => MemorySource::ToolVerified,
    }
}

async fn store(mem: &impl Memory, key: &str, content: &str, category: MemoryCategory) {
    let source = source_for_category(&category);
    mem.append_event(
        MemoryEventInput::new(
            "default",
            key,
            MemoryEventType::FactAdded,
            content,
            source,
            PrivacyLevel::Private,
        )
        .with_confidence(0.95)
        .with_importance(0.6),
    )
    .await
    .unwrap();
}

async fn count(mem: &impl Memory) -> usize {
    mem.count_events(None).await.unwrap()
}

async fn get_value(mem: &impl Memory, key: &str) -> Option<String> {
    mem.resolve_slot("default", key)
        .await
        .unwrap()
        .map(|slot| slot.value)
}

async fn recall(mem: &impl Memory, query: &str, limit: usize) -> Vec<(String, String, f64)> {
    mem.recall_scoped(RecallQuery::new("default", query, limit))
        .await
        .unwrap()
        .into_iter()
        .map(|item| (item.slot_key, item.value, item.score))
        .collect()
}

async fn forget(mem: &impl Memory, key: &str) -> bool {
    mem.forget_slot("default", key, ForgetMode::Hard, "comparison")
        .await
        .unwrap()
        .applied
}

// ── Test 1: Store performance ──────────────────────────────────

#[tokio::test]
async fn compare_store_speed() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();
    let sq = sqlite_backend(tmp_sq.path());
    let md = markdown_backend(tmp_md.path());

    let n = 100;

    // SQLite: 100 stores
    let start = Instant::now();
    for i in 0..n {
        store(
            &sq,
            &format!("key_{i}"),
            &format!("Memory entry number {i} about Rust programming"),
            MemoryCategory::Core,
        )
        .await;
    }
    let sq_dur = start.elapsed();

    // Markdown: 100 stores
    let start = Instant::now();
    for i in 0..n {
        store(
            &md,
            &format!("key_{i}"),
            &format!("Memory entry number {i} about Rust programming"),
            MemoryCategory::Core,
        )
        .await;
    }
    let md_dur = start.elapsed();

    println!("\n============================================================");
    println!("STORE {n} entries:");
    println!("  SQLite:   {:?}", sq_dur);
    println!("  Markdown: {:?}", md_dur);

    // Both should succeed
    assert_eq!(count(&sq).await, n);
    // Markdown count parses lines, may differ slightly from n
    let md_count = count(&md).await;
    assert!(md_count >= n, "Markdown stored {md_count}, expected >= {n}");
}

// ── Test 2: Recall / search quality ────────────────────────────

#[tokio::test]
async fn compare_recall_quality() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();
    let sq = sqlite_backend(tmp_sq.path());
    let md = markdown_backend(tmp_md.path());

    // Seed both with identical data
    let entries = vec![
        (
            "lang_pref",
            "User prefers Rust over Python",
            MemoryCategory::Core,
        ),
        (
            "editor",
            "Uses VS Code with rust-analyzer",
            MemoryCategory::Core,
        ),
        ("tz", "Timezone is EST, works 9-5", MemoryCategory::Core),
        (
            "proj1",
            "Working on AsteronIris AI assistant",
            MemoryCategory::Daily,
        ),
        (
            "proj2",
            "Previous project was a web scraper in Python",
            MemoryCategory::Daily,
        ),
        (
            "deploy",
            "Deploys to Hetzner VPS via Docker",
            MemoryCategory::Core,
        ),
        (
            "model",
            "Prefers Claude Sonnet for coding tasks",
            MemoryCategory::Core,
        ),
        (
            "style",
            "Likes concise responses, no fluff",
            MemoryCategory::Core,
        ),
        (
            "rust_note",
            "Rust's ownership model prevents memory bugs",
            MemoryCategory::Daily,
        ),
        (
            "perf",
            "Cares about binary size and startup time",
            MemoryCategory::Core,
        ),
    ];

    for (key, content, cat) in &entries {
        store(&sq, key, content, cat.clone()).await;
        store(&md, key, content, cat.clone()).await;
    }

    // Test queries and compare results
    let queries = vec![
        ("Rust", "Should find Rust-related entries"),
        ("Python", "Should find Python references"),
        ("deploy Docker", "Multi-keyword search"),
        ("Claude", "Specific tool reference"),
        ("javascript", "No matches expected"),
        ("binary size startup", "Multi-keyword partial match"),
    ];

    println!("\n============================================================");
    println!("RECALL QUALITY (10 entries seeded):\n");

    for (query, desc) in &queries {
        let sq_results = recall(&sq, query, 10).await;
        let md_results = recall(&md, query, 10).await;

        println!("  Query: \"{query}\" — {desc}");
        println!("    SQLite:   {} results", sq_results.len());
        for r in &sq_results {
            println!("      [{:.2}] {}: {}", r.2, r.0, &r.1[..r.1.len().min(50)]);
        }
        println!("    Markdown: {} results", md_results.len());
        for r in &md_results {
            println!("      [{:.2}] {}: {}", r.2, r.0, &r.1[..r.1.len().min(50)]);
        }
        println!();
    }
}

// ── Test 3: Recall speed at scale ──────────────────────────────

#[tokio::test]
async fn compare_recall_speed() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();
    let sq = sqlite_backend(tmp_sq.path());
    let md = markdown_backend(tmp_md.path());

    // Seed 200 entries
    let n = 200;
    for i in 0..n {
        let content = if i % 3 == 0 {
            format!("Rust is great for systems programming, entry {i}")
        } else if i % 3 == 1 {
            format!("Python is popular for data science, entry {i}")
        } else {
            format!("TypeScript powers modern web apps, entry {i}")
        };
        store(&sq, &format!("e{i}"), &content, MemoryCategory::Core).await;
        store(&md, &format!("e{i}"), &content, MemoryCategory::Daily).await;
    }

    // Benchmark recall
    let start = Instant::now();
    let sq_results = recall(&sq, "Rust", 10).await;
    let sq_dur = start.elapsed();

    let start = Instant::now();
    let md_results = recall(&md, "Rust", 10).await;
    let md_dur = start.elapsed();

    println!("\n============================================================");
    println!("RECALL from {n} entries (query: \"Rust\", limit 10):");
    println!("  SQLite:   {:?} → {} results", sq_dur, sq_results.len());
    println!("  Markdown: {:?} → {} results", md_dur, md_results.len());

    // Both should find results
    assert!(!sq_results.is_empty());
    assert!(!md_results.is_empty());
}

// ── Test 4: Persistence (SQLite wins by design) ────────────────

#[tokio::test]
async fn compare_persistence() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();

    // Store in both, then drop and re-open
    {
        let sq = sqlite_backend(tmp_sq.path());
        store(
            &sq,
            "persist_test",
            "I should survive",
            MemoryCategory::Core,
        )
        .await;
    }
    {
        let md = markdown_backend(tmp_md.path());
        store(
            &md,
            "persist_test",
            "I should survive",
            MemoryCategory::Core,
        )
        .await;
    }

    // Re-open
    let sq2 = sqlite_backend(tmp_sq.path());
    let md2 = markdown_backend(tmp_md.path());

    let sq_entry = get_value(&sq2, "persist_test").await;
    let md_entry = get_value(&md2, "persist_test").await;

    println!("\n============================================================");
    println!("PERSISTENCE (store → drop → re-open → get):");
    println!(
        "  SQLite:   {}",
        if sq_entry.is_some() {
            "✅ Survived"
        } else {
            "❌ Lost"
        }
    );
    println!(
        "  Markdown: {}",
        if md_entry.is_some() {
            "✅ Survived"
        } else {
            "❌ Lost"
        }
    );

    // SQLite should always persist by key
    assert!(sq_entry.is_some());
    assert_eq!(sq_entry.unwrap(), "I should survive");

    // Markdown persists content to files (get uses content search)
    assert!(md_entry.is_some());
}

// ── Test 5: Upsert / update behavior ──────────────────────────

#[tokio::test]
async fn compare_upsert() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();
    let sq = sqlite_backend(tmp_sq.path());
    let md = markdown_backend(tmp_md.path());

    // Store twice with same key, different content
    store(&sq, "pref", "likes Rust", MemoryCategory::Core).await;
    store(&sq, "pref", "loves Rust", MemoryCategory::Core).await;

    store(&md, "pref", "likes Rust", MemoryCategory::Core).await;
    store(&md, "pref", "loves Rust", MemoryCategory::Core).await;

    let sq_count = count(&sq).await;
    let md_count = count(&md).await;

    let sq_entry = get_value(&sq, "pref").await;
    let md_results = recall(&md, "loves Rust", 5).await;

    println!("\n============================================================");
    println!("UPSERT (store same key twice):");
    println!(
        "  SQLite:   count={sq_count}, latest=\"{}\"",
        sq_entry.as_deref().unwrap_or("none")
    );
    println!("  Markdown: count={md_count} (append-only, both entries kept)");
    println!("    Can still find latest: {}", !md_results.is_empty());

    assert_eq!(sq_count, 2);
    assert_eq!(sq_entry.unwrap(), "loves Rust");

    // Markdown: append-only, count increases
    assert!(md_count >= 2, "Markdown should keep both entries");
}

// ── Test 6: Forget / delete capability ─────────────────────────

#[tokio::test]
async fn compare_forget() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();
    let sq = sqlite_backend(tmp_sq.path());
    let md = markdown_backend(tmp_md.path());

    store(&sq, "secret", "API key: sk-1234", MemoryCategory::Core).await;
    store(&md, "secret", "API key: sk-1234", MemoryCategory::Core).await;

    let sq_forgot = forget(&sq, "secret").await;
    let md_forgot = forget(&md, "secret").await;

    println!("\n============================================================");
    println!("FORGET (delete sensitive data):");
    println!(
        "  SQLite:   {} (count={})",
        if sq_forgot { "✅ Deleted" } else { "❌ Kept" },
        count(&sq).await
    );
    println!(
        "  Markdown: {} (append-only by design)",
        if md_forgot {
            "✅ Deleted"
        } else {
            "⚠️  Cannot delete (audit trail)"
        },
    );

    // SQLite can delete
    assert!(sq_forgot);
    assert_eq!(count(&sq).await, 1);

    assert!(!md_forgot);
}

// ── Test 7: Category filtering ─────────────────────────────────

#[tokio::test]
async fn compare_category_filter() {
    let tmp_sq = TempDir::new().unwrap();
    let tmp_md = TempDir::new().unwrap();
    let sq = sqlite_backend(tmp_sq.path());
    let md = markdown_backend(tmp_md.path());

    // Mix of categories
    store(&sq, "a", "core fact 1", MemoryCategory::Core).await;
    store(&sq, "b", "core fact 2", MemoryCategory::Core).await;
    store(&sq, "c", "daily note", MemoryCategory::Daily).await;
    store(&sq, "d", "convo msg", MemoryCategory::Conversation).await;

    store(&md, "a", "core fact 1", MemoryCategory::Core).await;
    store(&md, "b", "core fact 2", MemoryCategory::Core).await;
    store(&md, "c", "daily note", MemoryCategory::Daily).await;

    let sq_slots = [
        sq.resolve_slot("default", "a").await.unwrap().unwrap(),
        sq.resolve_slot("default", "b").await.unwrap().unwrap(),
        sq.resolve_slot("default", "c").await.unwrap().unwrap(),
        sq.resolve_slot("default", "d").await.unwrap().unwrap(),
    ];
    let sq_core = sq_slots
        .iter()
        .filter(|slot| matches!(slot.source, MemorySource::ExplicitUser))
        .count();
    let sq_daily = sq_slots
        .iter()
        .filter(|slot| matches!(slot.source, MemorySource::System))
        .count();
    let sq_conv = sq_slots
        .iter()
        .filter(|slot| matches!(slot.source, MemorySource::Inferred))
        .count();
    let sq_all = count(&sq).await;

    let md_a = md.resolve_slot("default", "a").await.unwrap();
    let md_b = md.resolve_slot("default", "b").await.unwrap();
    let md_c = md.resolve_slot("default", "c").await.unwrap();
    let md_core = usize::from(md_a.is_some()) + usize::from(md_b.is_some());
    let md_daily = usize::from(md_c.is_some());
    let md_all = count(&md).await;

    println!("\n============================================================");
    println!("CATEGORY FILTERING:");
    println!(
        "  SQLite:   core={}, daily={}, conv={}, all={}",
        sq_core, sq_daily, sq_conv, sq_all
    );
    println!(
        "  Markdown: core={}, daily={}, all={}",
        md_core, md_daily, md_all
    );

    // SQLite: precise category filtering via SQL WHERE
    assert_eq!(sq_core, 2);
    assert_eq!(sq_daily, 1);
    assert_eq!(sq_conv, 1);
    assert_eq!(sq_all, 4);

    // Markdown: categories determined by file location
    assert!(md_core >= 1);
    assert!(md_all >= 1);
}
