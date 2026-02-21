use asteroniris::intelligence::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryInferenceEvent, MemorySource, PrivacyLevel,
    RecallQuery, SqliteMemory,
};
use chrono::{Duration, Utc};
use tempfile::TempDir;

#[tokio::test]
async fn sqlite_retrieval_salience_decay_ordering() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    let now = Utc::now();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.preference.language",
                MemoryEventType::FactAdded,
                "Current preference: Rust language for backend work",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.96)
            .with_importance(0.95)
            .with_occurred_at(now.to_rfc3339()),
        )
        .await
        .expect("fresh high-salience insert");

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.preference.legacy",
                MemoryEventType::FactAdded,
                "Legacy preference: Rust language for backend work",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_confidence(0.25)
            .with_importance(0.20)
            .with_occurred_at((now - Duration::days(140)).to_rfc3339()),
        )
        .await
        .expect("stale low-salience insert");

    let recalled = memory
        .recall_scoped(RecallQuery::new("default", "Rust language", 10))
        .await
        .expect("recall succeeds");

    let fresh_idx = recalled
        .iter()
        .position(|item| item.slot_key == "profile.preference.language")
        .expect("fresh memory present");
    let stale_idx = recalled
        .iter()
        .position(|item| item.slot_key == "profile.preference.legacy")
        .expect("stale memory present");

    assert!(
        fresh_idx < stale_idx,
        "fresh high-salience memory must outrank stale low-salience memory"
    );
}

#[tokio::test]
async fn sqlite_contradiction_penalty_affects_order() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    let now = Utc::now();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone.conflicted",
                MemoryEventType::FactAdded,
                "Timezone for meetings is UTC",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.95)
            .with_occurred_at(now.to_rfc3339()),
        )
        .await
        .expect("conflicted candidate insert");

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone.clean",
                MemoryEventType::FactAdded,
                "Timezone for meetings is UTC",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.90)
            .with_importance(0.90)
            .with_occurred_at((now - Duration::minutes(2)).to_rfc3339()),
        )
        .await
        .expect("clean baseline insert");

    memory
        .append_inference_event(
            MemoryInferenceEvent::contradiction_marked(
                "default",
                "profile.timezone.conflicted",
                "Detected contradiction against newer schedule metadata",
            )
            .with_confidence(1.0)
            .with_importance(1.0)
            .with_occurred_at((now + Duration::minutes(1)).to_rfc3339()),
        )
        .await
        .expect("contradiction event insert");

    let recalled_first = memory
        .recall_scoped(RecallQuery::new("default", "Timezone for meetings", 10))
        .await
        .expect("first recall succeeds");
    let recalled_second = memory
        .recall_scoped(RecallQuery::new("default", "Timezone for meetings", 10))
        .await
        .expect("second recall succeeds");

    let clean_idx = recalled_first
        .iter()
        .position(|item| item.slot_key == "profile.timezone.clean")
        .expect("clean slot present");
    let conflicted_idx = recalled_first
        .iter()
        .position(|item| item.slot_key == "profile.timezone.conflicted")
        .expect("conflicted slot present");

    assert!(
        clean_idx < conflicted_idx,
        "contradiction penalty must demote conflicted memory"
    );

    let order_first: Vec<&str> = recalled_first
        .iter()
        .map(|item| item.slot_key.as_str())
        .collect();
    let order_second: Vec<&str> = recalled_second
        .iter()
        .map(|item| item.slot_key.as_str())
        .collect();
    assert_eq!(order_first, order_second, "ordering must be deterministic");
}
