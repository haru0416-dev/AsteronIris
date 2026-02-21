use super::RuntimeMemoryWriteContext;
use crate::intelligence::memory::traits::MemoryLayer;
use crate::intelligence::memory::{Memory, MemoryInferenceEvent, MemoryProvenance, MemorySource};
use crate::observability::Observer;
use crate::observability::traits::AutonomyLifecycleSignal;
use anyhow::{Context, Result};
use std::sync::Arc;

fn parse_inference_payload(line: &str) -> Option<(&str, &str)> {
    let (slot_key, value) = line.split_once("=>")?;
    let slot_key = slot_key.trim();
    let value = value.trim();
    if slot_key.is_empty() || value.is_empty() {
        return None;
    }
    Some((slot_key, value))
}

fn build_post_turn_inference_events(
    entity_id: &str,
    assistant_response: &str,
) -> Vec<MemoryInferenceEvent> {
    const INFERRED_PREFIX: &str = "INFERRED_CLAIM ";
    const CONTRADICTION_PREFIX: &str = "CONTRADICTION_EVENT ";

    assistant_response
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(payload) = line.strip_prefix(INFERRED_PREFIX) {
                let (slot_key, value) = parse_inference_payload(payload)?;
                return Some(
                    MemoryInferenceEvent::inferred_claim(entity_id, slot_key, value)
                        .with_layer(MemoryLayer::Semantic),
                );
            }
            if let Some(payload) = line.strip_prefix(CONTRADICTION_PREFIX) {
                let (slot_key, value) = parse_inference_payload(payload)?;
                return Some(
                    MemoryInferenceEvent::contradiction_marked(entity_id, slot_key, value)
                        .with_layer(MemoryLayer::Episodic),
                );
            }
            None
        })
        .collect()
}

fn inference_provenance_reference(event: &MemoryInferenceEvent) -> (&'static str, MemorySource) {
    match event {
        MemoryInferenceEvent::InferredClaim { .. } => {
            ("inference.post_turn.inferred_claim", MemorySource::Inferred)
        }
        MemoryInferenceEvent::ContradictionEvent { .. } => (
            "inference.post_turn.contradiction_event",
            MemorySource::System,
        ),
    }
}

pub(super) async fn run_post_turn_inference_pass(
    mem: &dyn Memory,
    write_context: &RuntimeMemoryWriteContext,
    assistant_response: &str,
    observer: &Arc<dyn Observer>,
) -> Result<()> {
    write_context.enforce_write_scope()?;
    let events = build_post_turn_inference_events(&write_context.entity_id, assistant_response);
    if events.is_empty() {
        return Ok(());
    }

    for event in &events {
        if matches!(event, MemoryInferenceEvent::ContradictionEvent { .. }) {
            observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::ContradictionDetected);
        }
    }

    for event in events {
        let (reference, source_class) = inference_provenance_reference(&event);
        let input = event
            .into_memory_event_input()
            .with_provenance(MemoryProvenance::source_reference(source_class, reference));
        mem.append_event(input)
            .await
            .context("append inferred memory event")?;
    }

    Ok(())
}
