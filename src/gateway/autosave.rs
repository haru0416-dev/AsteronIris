use crate::memory::traits::MemoryLayer;
use crate::memory::{MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel};
use crate::security::policy::TenantPolicyContext;

pub(super) const GATEWAY_AUTOSAVE_ENTITY_ID: &str = "default";

pub(super) fn gateway_runtime_policy_context() -> TenantPolicyContext {
    TenantPolicyContext::disabled()
}

pub(super) fn gateway_webhook_autosave_event(summary: String) -> MemoryEventInput {
    MemoryEventInput::new(
        GATEWAY_AUTOSAVE_ENTITY_ID,
        "external.gateway.webhook",
        MemoryEventType::FactAdded,
        summary,
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_layer(MemoryLayer::Working)
    .with_confidence(0.95)
    .with_importance(0.5)
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::ExplicitUser,
        "gateway.autosave.webhook",
    ))
}

pub(super) fn gateway_whatsapp_autosave_event(sender: &str, summary: String) -> MemoryEventInput {
    MemoryEventInput::new(
        GATEWAY_AUTOSAVE_ENTITY_ID,
        format!("external.whatsapp.{sender}"),
        MemoryEventType::FactAdded,
        summary,
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_layer(MemoryLayer::Working)
    .with_confidence(0.95)
    .with_importance(0.6)
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::ExplicitUser,
        "gateway.autosave.whatsapp",
    ))
}
