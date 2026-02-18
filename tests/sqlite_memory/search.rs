use super::temp_sqlite;
use asteroniris::memory::traits::Memory;
use asteroniris::memory::{
    MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, RecallQuery,
};

#[tokio::test]
async fn sqlite_search_returns_keyword_hits() {
    let (_tmp, mem) = temp_sqlite();
    mem.append_event(MemoryEventInput::new(
        "user-search",
        "notes.favorite_language",
        MemoryEventType::FactAdded,
        "Rust is great for systems programming",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    ))
    .await
    .expect("append");

    let results = mem
        .recall_scoped(RecallQuery::new("systems"))
        .await
        .expect("recall");
    assert!(!results.is_empty());
}

#[tokio::test]
async fn sqlite_search_limit_zero_returns_empty() {
    let (_tmp, mem) = temp_sqlite();
    let results = mem
        .recall_scoped(RecallQuery::new("anything").with_limit(0))
        .await
        .expect("recall");
    assert!(results.is_empty());
}
