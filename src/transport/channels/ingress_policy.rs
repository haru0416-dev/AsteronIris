use crate::memory::{
    MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel, SourceKind,
};
use crate::persona::person_identity::channel_person_entity_id;
use crate::security::policy::TenantPolicyContext;

// TODO: Port security::external_content module to v2.
// For now, provide minimal stubs for the external content policy functions.

#[derive(Debug, Clone)]
pub(crate) struct ExternalIngressPolicyOutcome {
    pub model_input: String,
    pub persisted_summary: String,
    pub blocked: bool,
}

/// Apply external ingress policy to incoming channel content.
///
/// NOTE: In v2, the `security::external_content` module is not yet ported.
/// This stub passes content through with basic wrapping. Once the external
/// content module is available, this should delegate to `prepare_external_content`.
pub(crate) fn apply_external_ingress_policy(
    source: &str,
    text: &str,
) -> ExternalIngressPolicyOutcome {
    // Minimal stub: wrap the content with a source marker for the model,
    // and use the raw text as the persisted summary.
    let tag = source.replace(':', "_");
    let model_input = format!("[[external-content:{tag}]]{text}[[/external-content]]");

    ExternalIngressPolicyOutcome {
        model_input,
        persisted_summary: text.to_string(),
        blocked: false,
    }
}

pub(crate) fn channel_autosave_entity_id(channel: &str, sender: &str) -> String {
    channel_person_entity_id(channel, sender)
}

pub(crate) fn channel_runtime_policy_context() -> TenantPolicyContext {
    TenantPolicyContext::disabled()
}

pub(crate) fn channel_autosave_input(
    entity_id: &str,
    channel: &str,
    sender: &str,
    summary: String,
) -> MemoryEventInput {
    let source_kind = match channel {
        "discord" => SourceKind::Discord,
        "telegram" => SourceKind::Telegram,
        "slack" => SourceKind::Slack,
        "cli" => SourceKind::Conversation,
        _ => SourceKind::Api,
    };

    MemoryEventInput::new(
        entity_id,
        format!("external.channel.{channel}.{sender}"),
        MemoryEventType::FactAdded,
        summary,
        MemorySource::ExplicitUser,
        PrivacyLevel::Private,
    )
    .with_layer(crate::memory::MemoryLayer::Working)
    .with_confidence(0.95)
    .with_importance(0.6)
    .with_source_kind(source_kind)
    .with_source_ref(format!("channel:{channel}:{sender}"))
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::ExplicitUser,
        "channels.autosave.ingress",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_autosave_entity_id_is_person_scoped() {
        assert_eq!(
            channel_autosave_entity_id("discord", "u/123"),
            "person:discord.u_123"
        );
    }

    #[test]
    fn channel_autosave_input_sets_source_metadata_for_policy() {
        let input =
            channel_autosave_input("person:discord.u_1", "discord", "u/1", "hello".to_string());
        assert_eq!(input.source_kind, Some(SourceKind::Discord));
        assert_eq!(input.source_ref.as_deref(), Some("channel:discord:u/1"));
        assert!(input.provenance.is_some());
    }
}
