use crate::memory::{
    MemoryEventInput, MemoryEventType, MemoryLayer, MemoryProvenance, MemorySource, PrivacyLevel,
    SourceKind,
};
use crate::persona::channel_person_entity_id;
use crate::security::policy::TenantPolicyContext;

pub(super) fn gateway_autosave_entity_id(source: &str) -> String {
    channel_person_entity_id("gateway", source)
}

pub(super) fn gateway_runtime_policy_context() -> TenantPolicyContext {
    TenantPolicyContext::disabled()
}

pub(super) fn gateway_webhook_autosave_event(entity_id: &str, summary: String) -> MemoryEventInput {
    MemoryEventInput::new(
        entity_id,
        "external.gateway.webhook",
        MemoryEventType::FactAdded,
        summary,
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_layer(MemoryLayer::Working)
    .with_confidence(0.95)
    .with_importance(0.5)
    .with_source_kind(SourceKind::Api)
    .with_source_ref("gateway:webhook")
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::ExplicitUser,
        "gateway.autosave.webhook",
    ))
}

#[cfg(feature = "whatsapp")]
pub(super) fn gateway_whatsapp_autosave_event(
    entity_id: &str,
    sender: &str,
    summary: String,
) -> MemoryEventInput {
    MemoryEventInput::new(
        entity_id,
        format!("external.whatsapp.{sender}"),
        MemoryEventType::FactAdded,
        summary,
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_layer(MemoryLayer::Working)
    .with_confidence(0.95)
    .with_importance(0.6)
    .with_source_kind(SourceKind::Api)
    .with_source_ref(format!("gateway:whatsapp:{sender}"))
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::ExplicitUser,
        "gateway.autosave.whatsapp",
    ))
}
