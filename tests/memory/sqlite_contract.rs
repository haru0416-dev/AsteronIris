use super::memory_harness::sqlite_fixture as temp_sqlite;
use asteroniris::memory::traits::Memory;
use asteroniris::memory::{
    ForgetMode, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel,
};

#[tokio::test]
async fn sqlite_name_and_health_contract() {
    let (_tmp, mem) = temp_sqlite();
    assert_eq!(mem.name(), "sqlite");
    assert!(mem.health_check().await);
}

#[tokio::test]
async fn sqlite_append_recall_forget_contract() {
    let (_tmp, mem) = temp_sqlite();
    mem.append_event(MemoryEventInput::new(
        "user-1",
        "profile.color",
        MemoryEventType::FactAdded,
        "blue",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    ))
    .await
    .expect("append");

    let recalled = mem
        .recall_scoped(asteroniris::memory::RecallQuery::new("user-1", "blue", 10))
        .await
        .expect("recall");
    assert!(!recalled.is_empty());

    let outcome = mem
        .forget_slot("user-1", "profile.color", ForgetMode::Soft, "contract-test")
        .await
        .expect("forget");
    assert_eq!(outcome.mode, ForgetMode::Soft);
}
