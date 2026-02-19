use super::memory_harness::append_test_event;
use super::memory_harness::sqlite_fixture;
use asteroniris::memory::{
    Memory, MemoryCategory, MemoryEventInput, MemoryEventType, MemoryInferenceEvent, MemorySource,
    PrivacyLevel,
};
use rusqlite::Connection;

#[tokio::test]
async fn memory_inferred_claim_persists() {
    let (_tmp, memory) = sqlite_fixture();

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
    let (_tmp, memory) = sqlite_fixture();

    append_test_event(
        &memory,
        "default",
        "profile.timezone",
        "UTC+9",
        MemoryCategory::Core,
    )
    .await;

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

#[tokio::test]
async fn memory_conflict_resolver_precedence() {
    let (tmp, memory) = sqlite_fixture();

    let explicit = memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone",
                MemoryEventType::FactAdded,
                "UTC+9",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.82)
            .with_importance(0.7)
            .with_occurred_at("2026-01-15T20:00:00+09:00"),
        )
        .await
        .unwrap();

    memory
        .append_inference_event(
            MemoryInferenceEvent::inferred_claim("default", "profile.timezone", "UTC")
                .with_confidence(1.0)
                .with_occurred_at("2026-01-16T12:00:00Z"),
        )
        .await
        .unwrap();

    let after_inferred = memory
        .resolve_slot("default", "profile.timezone")
        .await
        .unwrap()
        .expect("slot should remain resolvable");
    assert_eq!(after_inferred.value, "UTC+9");
    assert_eq!(after_inferred.source, MemorySource::ExplicitUser);

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone",
                MemoryEventType::FactUpdated,
                "UTC+1",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.70)
            .with_occurred_at("2026-01-17T00:00:00Z"),
        )
        .await
        .unwrap();

    let after_lower_confidence = memory
        .resolve_slot("default", "profile.timezone")
        .await
        .unwrap()
        .expect("slot should remain resolvable");
    assert_eq!(after_lower_confidence.value, "UTC+9");

    let explicit_high_confidence = memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone",
                MemoryEventType::FactUpdated,
                "UTC+8",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.97)
            .with_occurred_at("2026-01-15T00:00:00Z"),
        )
        .await
        .unwrap();

    let winner = memory
        .resolve_slot("default", "profile.timezone")
        .await
        .unwrap()
        .expect("slot should remain resolvable");
    assert_eq!(winner.value, "UTC+8");
    assert_eq!(winner.source, MemorySource::ExplicitUser);

    let contradiction = memory
        .append_inference_event(MemoryInferenceEvent::contradiction_marked(
            "default",
            "profile.timezone",
            "Conflict detected",
        ))
        .await
        .unwrap();

    let conn = Connection::open(tmp.path().join("memory").join("brain.db")).unwrap();
    let supersedes_event_id: Option<String> = conn
        .query_row(
            "SELECT supersedes_event_id FROM memory_events WHERE event_id = ?1",
            [&contradiction.event_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        supersedes_event_id.as_deref(),
        Some(explicit_high_confidence.event_id.as_str())
    );
    assert_ne!(explicit.event_id, explicit_high_confidence.event_id);
}

#[tokio::test]
async fn memory_conflict_resolver_timestamp_normalization() {
    let (_tmp, memory) = sqlite_fixture();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.locale",
                MemoryEventType::FactAdded,
                "ko-KR",
                MemorySource::ToolVerified,
                PrivacyLevel::Private,
            )
            .with_confidence(0.90)
            .with_occurred_at("2026-01-15T23:00:00+09:00"),
        )
        .await
        .unwrap();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.locale",
                MemoryEventType::FactUpdated,
                "en-US",
                MemorySource::ToolVerified,
                PrivacyLevel::Private,
            )
            .with_confidence(0.90)
            .with_occurred_at("2026-01-15T15:30:00+00:00"),
        )
        .await
        .unwrap();

    let after_newer_instant = memory
        .resolve_slot("default", "profile.locale")
        .await
        .unwrap()
        .expect("slot should be resolvable");
    assert_eq!(after_newer_instant.value, "en-US");

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.locale",
                MemoryEventType::FactUpdated,
                "fr-FR",
                MemorySource::ToolVerified,
                PrivacyLevel::Private,
            )
            .with_confidence(0.90)
            .with_occurred_at("2026-01-16T00:30:00+09:00"),
        )
        .await
        .unwrap();

    let stable_winner = memory
        .resolve_slot("default", "profile.locale")
        .await
        .unwrap()
        .expect("slot should be resolvable");
    assert_eq!(stable_winner.value, "en-US");
}
