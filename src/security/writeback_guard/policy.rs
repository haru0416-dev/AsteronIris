use crate::memory::{MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SourceKind};
use anyhow::Context as _;

fn expected_person_entity(person_id: &str) -> String {
    format!("person:{person_id}")
}

fn require_common_write_metadata(event: &MemoryEventInput) -> anyhow::Result<()> {
    let Some(source_ref) = event.source_ref.as_deref() else {
        anyhow::bail!("write policy requires source_ref");
    };
    if source_ref.trim().is_empty() {
        anyhow::bail!("write policy source_ref must not be empty");
    }

    let Some(provenance) = event.provenance.as_ref() else {
        anyhow::bail!("write policy requires provenance");
    };
    if provenance.source_class != event.source {
        anyhow::bail!("write policy requires provenance.source_class to match source");
    }
    if provenance.reference.trim().is_empty() {
        anyhow::bail!("write policy requires provenance.reference");
    }

    Ok(())
}

pub fn enforce_persona_long_term_write_policy(
    event: &MemoryEventInput,
    person_id: &str,
) -> anyhow::Result<()> {
    if event.source != MemorySource::System {
        anyhow::bail!("persona writeback policy requires source=system");
    }

    if event.privacy_level != PrivacyLevel::Private {
        anyhow::bail!("persona writeback policy requires privacy_level=private");
    }

    if event.source_kind != Some(SourceKind::Manual) {
        anyhow::bail!("persona writeback policy requires source_kind=manual");
    }

    require_common_write_metadata(event)?;
    let provenance = event
        .provenance
        .as_ref()
        .context("provenance missing after validation")?;
    if provenance.source_class != MemorySource::System {
        anyhow::bail!("persona writeback policy requires provenance.source_class=system");
    }

    if event.entity_id != expected_person_entity(person_id) {
        anyhow::bail!("persona writeback policy entity_id mismatch");
    }

    if event.slot_key.starts_with("persona.writeback.") {
        if event.event_type != MemoryEventType::SummaryCompacted {
            anyhow::bail!("persona writeback entries must use event_type=summary_compacted");
        }
        return Ok(());
    }

    let canonical_prefix = format!("persona/{person_id}/state_header/");
    if event.slot_key.starts_with(&canonical_prefix) {
        if event.event_type != MemoryEventType::FactUpdated {
            anyhow::bail!("persona canonical state writes must use event_type=fact_updated");
        }
        return Ok(());
    }

    anyhow::bail!("persona writeback policy rejected slot_key");
}

pub fn enforce_tool_memory_write_policy(event: &MemoryEventInput) -> anyhow::Result<()> {
    if event.privacy_level == PrivacyLevel::Secret {
        anyhow::bail!("tool memory write policy rejects privacy_level=secret");
    }
    if event.source_kind != Some(SourceKind::Manual) {
        anyhow::bail!("tool memory write policy requires source_kind=manual");
    }
    require_common_write_metadata(event)
}

pub fn enforce_external_autosave_write_policy(event: &MemoryEventInput) -> anyhow::Result<()> {
    if event.source != MemorySource::ExplicitUser {
        anyhow::bail!("external autosave policy requires source=explicit_user");
    }
    if event.privacy_level != PrivacyLevel::Private {
        anyhow::bail!("external autosave policy requires privacy_level=private");
    }
    let Some(source_kind) = event.source_kind else {
        anyhow::bail!("external autosave policy requires source_kind");
    };
    match source_kind {
        SourceKind::Api
        | SourceKind::Conversation
        | SourceKind::Discord
        | SourceKind::Telegram
        | SourceKind::Slack => {}
        _ => anyhow::bail!("external autosave policy rejected source_kind"),
    }
    require_common_write_metadata(event)
}

pub fn enforce_agent_autosave_write_policy(event: &MemoryEventInput) -> anyhow::Result<()> {
    if event.privacy_level != PrivacyLevel::Private {
        anyhow::bail!("agent autosave policy requires privacy_level=private");
    }
    if event.source_kind != Some(SourceKind::Conversation) {
        anyhow::bail!("agent autosave policy requires source_kind=conversation");
    }
    if event.event_type != MemoryEventType::FactAdded {
        anyhow::bail!("agent autosave policy requires event_type=fact_added");
    }
    if event.slot_key != "conversation.user_msg" && event.slot_key != "conversation.assistant_resp"
    {
        anyhow::bail!("agent autosave policy rejected slot_key");
    }
    match event.source {
        MemorySource::ExplicitUser | MemorySource::System => {}
        _ => anyhow::bail!("agent autosave policy rejected source"),
    }
    require_common_write_metadata(event)
}

pub fn enforce_inference_write_policy(event: &MemoryEventInput) -> anyhow::Result<()> {
    if event.privacy_level != PrivacyLevel::Private {
        anyhow::bail!("inference write policy requires privacy_level=private");
    }
    if event.source_kind != Some(SourceKind::Conversation) {
        anyhow::bail!("inference write policy requires source_kind=conversation");
    }
    match event.source {
        MemorySource::Inferred | MemorySource::System => {}
        _ => anyhow::bail!("inference write policy rejected source"),
    }
    match event.event_type {
        MemoryEventType::InferredClaim | MemoryEventType::ContradictionMarked => {}
        _ => anyhow::bail!("inference write policy rejected event_type"),
    }
    require_common_write_metadata(event)
}

pub fn enforce_verify_repair_write_policy(event: &MemoryEventInput) -> anyhow::Result<()> {
    if event.source != MemorySource::System {
        anyhow::bail!("verify-repair write policy requires source=system");
    }
    if event.privacy_level != PrivacyLevel::Private {
        anyhow::bail!("verify-repair write policy requires privacy_level=private");
    }
    if event.source_kind != Some(SourceKind::Manual) {
        anyhow::bail!("verify-repair write policy requires source_kind=manual");
    }
    if event.slot_key != "autonomy.verify_repair.escalation" {
        anyhow::bail!("verify-repair write policy rejected slot_key");
    }
    if event.event_type != MemoryEventType::SummaryCompacted {
        anyhow::bail!("verify-repair write policy requires event_type=summary_compacted");
    }
    require_common_write_metadata(event)
}

pub fn enforce_ingestion_write_policy(event: &MemoryEventInput) -> anyhow::Result<()> {
    if event.event_type != MemoryEventType::FactAdded {
        anyhow::bail!("ingestion write policy requires event_type=fact_added");
    }
    match event.source {
        MemorySource::ExplicitUser
        | MemorySource::ExternalPrimary
        | MemorySource::ExternalSecondary => {}
        _ => anyhow::bail!("ingestion write policy rejected source"),
    }
    let Some(source_kind) = event.source_kind else {
        anyhow::bail!("ingestion write policy requires source_kind");
    };
    match source_kind {
        SourceKind::Conversation
        | SourceKind::Manual
        | SourceKind::Discord
        | SourceKind::Telegram
        | SourceKind::Slack
        | SourceKind::Api
        | SourceKind::News
        | SourceKind::Document => {}
    }
    if !event.slot_key.starts_with("external.") {
        anyhow::bail!("ingestion write policy requires external slot_key prefix");
    }
    require_common_write_metadata(event)
}
