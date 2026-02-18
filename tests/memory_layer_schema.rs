use asteroniris::memory::traits::MemoryLayer;
use asteroniris::memory::{
    MemoryEventInput, MemoryEventType, MemoryInferenceEvent, MemorySource, PrivacyLevel,
};

#[test]
fn memory_layer_serde_roundtrip() {
    let input = MemoryEventInput::new(
        "entity-1",
        "profile.locale",
        MemoryEventType::FactAdded,
        "en-US",
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_layer(MemoryLayer::Identity)
    .with_confidence(0.91)
    .with_importance(0.64)
    .with_occurred_at("2026-02-18T00:00:00Z");

    let input_json = serde_json::to_value(&input).expect("memory event input should serialize");
    assert_eq!(input_json["layer"], "identity");

    let input_roundtrip: MemoryEventInput =
        serde_json::from_value(input_json).expect("memory event input should deserialize");
    assert_eq!(input_roundtrip.layer, MemoryLayer::Identity);

    let inference = MemoryInferenceEvent::inferred_claim(
        "entity-1",
        "skills.rust",
        "prefers cargo over manual linking",
    )
    .with_layer(MemoryLayer::Procedural)
    .with_occurred_at("2026-02-18T00:00:00Z");

    let inference_json =
        serde_json::to_value(&inference).expect("inference event should serialize");
    assert_eq!(inference_json["layer"], "procedural");

    let inference_roundtrip: MemoryInferenceEvent =
        serde_json::from_value(inference_json).expect("inference event should deserialize");

    match &inference_roundtrip {
        MemoryInferenceEvent::InferredClaim { layer, .. } => {
            assert_eq!(*layer, MemoryLayer::Procedural);
        }
        _ => panic!("expected inferred claim variant"),
    }

    let projected = inference_roundtrip.into_memory_event_input();
    assert_eq!(projected.layer, MemoryLayer::Procedural);
}

#[test]
fn memory_layer_backward_compat() {
    let legacy_input = serde_json::json!({
        "entity_id": "entity-legacy",
        "slot_key": "profile.timezone",
        "event_type": "fact_added",
        "value": "UTC",
        "source": "explicit_user",
        "confidence": 0.8,
        "importance": 0.5,
        "privacy_level": "private",
        "occurred_at": "2026-02-18T00:00:00Z"
    });

    let parsed_input: MemoryEventInput =
        serde_json::from_value(legacy_input).expect("legacy event input should deserialize");
    assert_eq!(parsed_input.layer, MemoryLayer::Working);

    let legacy_inferred = serde_json::json!({
        "kind": "inferred_claim",
        "entity_id": "entity-legacy",
        "slot_key": "preferences.editor",
        "value": "neovim",
        "confidence": 0.7,
        "importance": 0.5,
        "privacy_level": "private",
        "occurred_at": "2026-02-18T00:00:00Z"
    });

    let parsed_inferred: MemoryInferenceEvent =
        serde_json::from_value(legacy_inferred).expect("legacy inferred claim should deserialize");
    match &parsed_inferred {
        MemoryInferenceEvent::InferredClaim { layer, .. } => {
            assert_eq!(*layer, MemoryLayer::Semantic);
        }
        _ => panic!("expected inferred claim variant"),
    }

    let inferred_layer = parsed_inferred.into_memory_event_input().layer;
    assert_eq!(inferred_layer, MemoryLayer::Semantic);

    let legacy_contradiction = serde_json::json!({
        "kind": "contradiction_event",
        "entity_id": "entity-legacy",
        "slot_key": "profile.timezone",
        "value": "conflict",
        "confidence": 0.85,
        "importance": 0.8,
        "privacy_level": "private",
        "occurred_at": "2026-02-18T00:00:00Z"
    });

    let parsed_contradiction: MemoryInferenceEvent = serde_json::from_value(legacy_contradiction)
        .expect("legacy contradiction event should deserialize");
    match &parsed_contradiction {
        MemoryInferenceEvent::ContradictionEvent { layer, .. } => {
            assert_eq!(*layer, MemoryLayer::Episodic);
        }
        _ => panic!("expected contradiction variant"),
    }

    let contradiction_layer = parsed_contradiction.into_memory_event_input().layer;
    assert_eq!(contradiction_layer, MemoryLayer::Episodic);
}
