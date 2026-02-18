use asteroniris::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
    SqliteMemory,
};
use tempfile::TempDir;

fn sqlite_memory() -> (TempDir, SqliteMemory) {
    let temp = TempDir::new().expect("temp dir should be created");
    let memory = SqliteMemory::new(temp.path()).expect("sqlite memory should initialize");
    (temp, memory)
}

#[tokio::test]
async fn memory_provenance_validation_accepts_valid() {
    let (_temp, memory) = sqlite_memory();
    let input = MemoryEventInput::new(
        "entity-1",
        "profile.locale",
        MemoryEventType::FactAdded,
        "ko-KR",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_provenance(
        MemoryProvenance::source_reference(MemorySource::ExplicitUser, "ticket:ULTRA-103")
            .with_evidence_uri("https://example.com/evidence/ULTRA-103"),
    )
    .with_importance(0.66);

    let event = memory
        .append_event(input)
        .await
        .expect("valid provenance payload should be accepted");

    assert_eq!(event.confidence, 0.95);
    assert_eq!(event.importance, 0.66);

    let provenance = event
        .provenance
        .expect("accepted payload should preserve provenance");
    assert_eq!(provenance.source_class, MemorySource::ExplicitUser);
    assert_eq!(provenance.reference, "ticket:ULTRA-103");
    assert_eq!(
        provenance.evidence_uri.as_deref(),
        Some("https://example.com/evidence/ULTRA-103")
    );
}

#[tokio::test]
async fn memory_provenance_validation_rejects_invalid() {
    let (_temp, memory) = sqlite_memory();

    let source_mismatch = MemoryEventInput::new(
        "entity-1",
        "profile.locale",
        MemoryEventType::FactAdded,
        "ko-KR",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::System,
        "trace:ingress-404",
    ));

    let mismatch_error = memory
        .append_event(source_mismatch)
        .await
        .expect_err("mismatched provenance source should be rejected");
    assert_eq!(
        mismatch_error.to_string(),
        "memory_event_input.provenance.source_class must match memory_event_input.source"
    );

    let empty_reference = MemoryEventInput::new(
        "entity-1",
        "profile.locale",
        MemoryEventType::FactAdded,
        "ko-KR",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::ExplicitUser,
        "   ",
    ));

    let empty_reference_error = memory
        .append_event(empty_reference)
        .await
        .expect_err("blank provenance reference should be rejected");
    assert_eq!(
        empty_reference_error.to_string(),
        "memory_event_input.provenance.reference must not be empty"
    );

    let not_finite_confidence = MemoryEventInput::new(
        "entity-1",
        "profile.locale",
        MemoryEventType::FactAdded,
        "ko-KR",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_confidence(f64::NAN);

    let confidence_error = memory
        .append_event(not_finite_confidence)
        .await
        .expect_err("non-finite confidence should be rejected");
    assert_eq!(
        confidence_error.to_string(),
        "memory_event_input.confidence must be finite"
    );
}

#[test]
fn memory_provenance_validation_defaults_confidence_by_source_class() {
    let explicit = MemoryEventInput::new(
        "entity-default",
        "slot-default",
        MemoryEventType::FactAdded,
        "value",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    );
    assert_eq!(explicit.confidence, 0.95);

    let tool_verified = MemoryEventInput::new(
        "entity-default",
        "slot-default",
        MemoryEventType::FactAdded,
        "value",
        MemorySource::ToolVerified,
        PrivacyLevel::Private,
    );
    assert_eq!(tool_verified.confidence, 0.9);

    let system = MemoryEventInput::new(
        "entity-default",
        "slot-default",
        MemoryEventType::FactAdded,
        "value",
        MemorySource::System,
        PrivacyLevel::Private,
    );
    assert_eq!(system.confidence, 0.8);

    let inferred = MemoryEventInput::new(
        "entity-default",
        "slot-default",
        MemoryEventType::FactAdded,
        "value",
        MemorySource::Inferred,
        PrivacyLevel::Private,
    );
    assert_eq!(inferred.confidence, 0.7);
}
