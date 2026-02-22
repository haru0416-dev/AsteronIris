use super::memory_harness::sqlite_fixture as temp_sqlite;
use asteroniris::core::memory::traits::Memory;
use asteroniris::core::memory::{
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
        .recall_scoped(RecallQuery::new("user-search", "systems", 10))
        .await
        .expect("recall");
    assert!(!results.is_empty());
}

#[tokio::test]
async fn sqlite_search_limit_zero_returns_empty() {
    let (_tmp, mem) = temp_sqlite();
    let results = mem
        .recall_scoped(RecallQuery::new("any-entity", "anything", 0))
        .await
        .expect("recall");
    assert!(results.is_empty());
}

#[tokio::test]
async fn sqlite_recall_phased_delegates_to_scoped() {
    let (_tmp, mem) = temp_sqlite();
    mem.append_event(MemoryEventInput::new(
        "user-phased",
        "notes.runtime",
        MemoryEventType::FactAdded,
        "phased recall test value",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    ))
    .await
    .expect("append");

    let scoped = mem
        .recall_scoped(RecallQuery::new("user-phased", "phased recall", 5))
        .await
        .expect("scoped recall");
    let phased = mem
        .recall_phased(RecallQuery::new("user-phased", "phased recall", 5))
        .await
        .expect("phased recall");

    assert_eq!(scoped.len(), phased.len());
    assert_eq!(
        scoped.first().map(|item| item.slot_key.as_str()),
        phased.first().map(|item| item.slot_key.as_str())
    );
}
