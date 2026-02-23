use super::*;
use std::fs as sync_fs;
use tempfile::TempDir;

fn temp_workspace() -> (TempDir, MarkdownMemory) {
    let tmp = TempDir::new().unwrap();
    let mem = MarkdownMemory::new(tmp.path());
    (tmp, mem)
}

#[tokio::test]
async fn markdown_name() {
    let (_tmp, mem) = temp_workspace();
    assert_eq!(mem.name(), "markdown");
}

#[tokio::test]
async fn markdown_health_check() {
    let (_tmp, mem) = temp_workspace();
    assert!(mem.health_check().await);
}

#[tokio::test]
async fn markdown_store_core() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "pref",
        "User likes Rust",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    let content = sync_fs::read_to_string(mem.core_path()).unwrap();
    assert!(content.contains("User likes Rust"));
}

#[tokio::test]
async fn markdown_store_daily() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "note",
        "Finished tests",
        MemoryCategory::Daily,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    let path = mem.daily_path();
    let content = sync_fs::read_to_string(path).unwrap();
    assert!(content.contains("Finished tests"));
}

#[tokio::test]
async fn markdown_recall_keyword() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "a",
        "Rust is fast",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    mem.upsert_projection_entry(
        "b",
        "Python is slow",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    mem.upsert_projection_entry(
        "c",
        "Rust and safety",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();

    let results = mem.search_projection("Rust", 10).await.unwrap();
    assert!(results.len() >= 2);
    assert!(
        results
            .iter()
            .all(|r| r.content.to_lowercase().contains("rust"))
    );
}

#[tokio::test]
async fn markdown_recall_no_match() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "a",
        "Rust is great",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    let results = mem.search_projection("javascript", 10).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn markdown_count() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "a",
        "first",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    mem.upsert_projection_entry(
        "b",
        "second",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    let count = mem.count_projection_entries().await.unwrap();
    assert!(count >= 2);
}

#[tokio::test]
async fn markdown_list_by_category() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "a",
        "core fact",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    mem.upsert_projection_entry(
        "b",
        "daily note",
        MemoryCategory::Daily,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();

    let core = mem
        .list_projection_entries(Some(&MemoryCategory::Core))
        .await
        .unwrap();
    assert!(core.iter().all(|e| e.category == MemoryCategory::Core));

    let daily = mem
        .list_projection_entries(Some(&MemoryCategory::Daily))
        .await
        .unwrap();
    assert!(daily.iter().all(|e| e.category == MemoryCategory::Daily));
}

#[tokio::test]
async fn markdown_forget_is_noop() {
    let (_tmp, mem) = temp_workspace();
    mem.upsert_projection_entry(
        "a",
        "permanent",
        MemoryCategory::Core,
        MemoryLayer::Working,
        None,
    )
    .await
    .unwrap();
    let removed = mem.delete_projection_entry("a").await.unwrap();
    assert!(!removed, "Markdown memory is append-only");
}

#[tokio::test]
async fn markdown_empty_recall() {
    let (_tmp, mem) = temp_workspace();
    let results = mem.search_projection("anything", 10).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn markdown_empty_count() {
    let (_tmp, mem) = temp_workspace();
    assert_eq!(mem.count_projection_entries().await.unwrap(), 0);
}
