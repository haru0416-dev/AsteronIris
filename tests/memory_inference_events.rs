use asteroniris::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryInferenceEvent, MemorySource, PrivacyLevel,
    SqliteMemory,
};
use tempfile::TempDir;

#[tokio::test]
async fn memory_inferred_claim_persists() {
    let tmp = TempDir::new().unwrap();
    let memory = SqliteMemory::new(tmp.path()).unwrap();

    let events = memory
        .append_inference_events(vec![MemoryInferenceEvent::inferred_claim(
            "default",
            "persona.preference.language",
            "User prefers Rust",
        )])
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.to_string(), "inferred_claim");

    let slot = memory
        .resolve_slot("default", "persona.preference.language")
        .await
        .unwrap()
        .expect("inferred slot should be available");
    assert_eq!(slot.value, "User prefers Rust");
    assert_eq!(slot.source, MemorySource::Inferred);
}

#[tokio::test]
async fn memory_contradiction_event_recorded() {
    let tmp = TempDir::new().unwrap();
    let memory = SqliteMemory::new(tmp.path()).unwrap();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone",
                MemoryEventType::FactAdded,
                "UTC+9",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.8),
        )
        .await
        .unwrap();

    let events = memory
        .append_inference_events(vec![MemoryInferenceEvent::contradiction_marked(
            "default",
            "profile.timezone",
            "Conflict detected: prior=UTC+9 incoming=UTC",
        )])
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type.to_string(), "contradiction_marked");

    let slot = memory
        .resolve_slot("default", "profile.timezone")
        .await
        .unwrap()
        .expect("existing explicit slot must remain");
    assert_eq!(slot.value, "UTC+9");
    assert_eq!(slot.source, MemorySource::ExplicitUser);
}
