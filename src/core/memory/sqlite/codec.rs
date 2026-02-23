use super::SqliteMemory;
use crate::core::memory::traits::MemoryLayer;
use crate::core::memory::{MemorySource, PrivacyLevel, SignalTier, SourceKind};

impl SqliteMemory {
    pub(super) fn source_to_str(source: MemorySource) -> &'static str {
        match source {
            MemorySource::ExplicitUser => "explicit_user",
            MemorySource::ToolVerified => "tool_verified",
            MemorySource::System => "system",
            MemorySource::Inferred => "inferred",
            MemorySource::ExternalPrimary => "external_primary",
            MemorySource::ExternalSecondary => "external_secondary",
        }
    }

    pub(super) fn signal_tier_to_str(tier: SignalTier) -> &'static str {
        match tier {
            SignalTier::Raw => "raw",
            SignalTier::Belief => "belief",
            SignalTier::Inferred => "inferred",
            SignalTier::Governance => "governance",
        }
    }

    pub(super) fn str_to_signal_tier(s: &str) -> SignalTier {
        match s {
            "belief" => SignalTier::Belief,
            "inferred" => SignalTier::Inferred,
            "governance" => SignalTier::Governance,
            _ => SignalTier::Raw,
        }
    }

    pub(super) fn source_kind_to_str(kind: SourceKind) -> &'static str {
        match kind {
            SourceKind::Conversation => "conversation",
            SourceKind::Discord => "discord",
            SourceKind::Telegram => "telegram",
            SourceKind::Slack => "slack",
            SourceKind::Api => "api",
            SourceKind::News => "news",
            SourceKind::Document => "document",
            SourceKind::Manual => "manual",
        }
    }

    pub(super) fn str_to_source_kind(s: &str) -> Option<SourceKind> {
        match s {
            "conversation" => Some(SourceKind::Conversation),
            "discord" => Some(SourceKind::Discord),
            "telegram" => Some(SourceKind::Telegram),
            "slack" => Some(SourceKind::Slack),
            "api" => Some(SourceKind::Api),
            "news" => Some(SourceKind::News),
            "document" => Some(SourceKind::Document),
            "manual" => Some(SourceKind::Manual),
            _ => None,
        }
    }

    pub(super) fn layer_to_str(layer: MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    pub(super) fn retention_tier_for_layer(layer: MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    pub(super) fn retention_expiry_for_layer(
        layer: MemoryLayer,
        occurred_at: &str,
    ) -> Option<String> {
        let retention_days = match layer {
            MemoryLayer::Working => Some(2),
            MemoryLayer::Episodic => Some(30),
            MemoryLayer::Semantic | MemoryLayer::Procedural | MemoryLayer::Identity => None,
        }?;

        chrono::DateTime::parse_from_rfc3339(occurred_at)
            .ok()
            .map(|ts| (ts + chrono::Duration::days(retention_days)).to_rfc3339())
    }

    pub(super) fn str_to_source(source: &str) -> MemorySource {
        match source {
            "explicit_user" => MemorySource::ExplicitUser,
            "tool_verified" => MemorySource::ToolVerified,
            "inferred" => MemorySource::Inferred,
            "external_primary" => MemorySource::ExternalPrimary,
            "external_secondary" => MemorySource::ExternalSecondary,
            _ => MemorySource::System,
        }
    }

    pub(super) fn privacy_to_str(level: &PrivacyLevel) -> &'static str {
        match level {
            PrivacyLevel::Public => "public",
            PrivacyLevel::Private => "private",
            PrivacyLevel::Secret => "secret",
        }
    }

    pub(super) fn str_to_privacy(level: &str) -> PrivacyLevel {
        match level {
            "public" => PrivacyLevel::Public,
            "secret" => PrivacyLevel::Secret,
            _ => PrivacyLevel::Private,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_layers() -> [MemoryLayer; 5] {
        [
            MemoryLayer::Working,
            MemoryLayer::Episodic,
            MemoryLayer::Semantic,
            MemoryLayer::Procedural,
            MemoryLayer::Identity,
        ]
    }

    #[test]
    fn source_round_trip_for_all_variants() {
        let sources = [
            MemorySource::ExplicitUser,
            MemorySource::ToolVerified,
            MemorySource::System,
            MemorySource::Inferred,
            MemorySource::ExternalPrimary,
            MemorySource::ExternalSecondary,
        ];

        for source in sources {
            let encoded = SqliteMemory::source_to_str(source);
            let decoded = SqliteMemory::str_to_source(encoded);
            assert_eq!(decoded, source);
        }
    }

    #[test]
    fn layer_to_str_maps_all_variants() {
        assert_eq!(SqliteMemory::layer_to_str(MemoryLayer::Working), "working");
        assert_eq!(
            SqliteMemory::layer_to_str(MemoryLayer::Episodic),
            "episodic"
        );
        assert_eq!(
            SqliteMemory::layer_to_str(MemoryLayer::Semantic),
            "semantic"
        );
        assert_eq!(
            SqliteMemory::layer_to_str(MemoryLayer::Procedural),
            "procedural"
        );
        assert_eq!(
            SqliteMemory::layer_to_str(MemoryLayer::Identity),
            "identity"
        );
    }

    #[test]
    fn retention_tier_for_layer_maps_all_variants() {
        assert_eq!(
            SqliteMemory::retention_tier_for_layer(MemoryLayer::Working),
            "working"
        );
        assert_eq!(
            SqliteMemory::retention_tier_for_layer(MemoryLayer::Episodic),
            "episodic"
        );
        assert_eq!(
            SqliteMemory::retention_tier_for_layer(MemoryLayer::Semantic),
            "semantic"
        );
        assert_eq!(
            SqliteMemory::retention_tier_for_layer(MemoryLayer::Procedural),
            "procedural"
        );
        assert_eq!(
            SqliteMemory::retention_tier_for_layer(MemoryLayer::Identity),
            "identity"
        );
    }

    #[test]
    fn retention_expiry_for_layer_maps_expected_windows() {
        let occurred_at = "2026-01-01T00:00:00+00:00";
        let occurred = chrono::DateTime::parse_from_rfc3339(occurred_at).unwrap();

        let working_expiry =
            SqliteMemory::retention_expiry_for_layer(MemoryLayer::Working, occurred_at)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok());
        assert_eq!(working_expiry, Some(occurred + chrono::Duration::days(2)));

        let episodic_expiry =
            SqliteMemory::retention_expiry_for_layer(MemoryLayer::Episodic, occurred_at)
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok());
        assert_eq!(episodic_expiry, Some(occurred + chrono::Duration::days(30)));

        for layer in [
            MemoryLayer::Semantic,
            MemoryLayer::Procedural,
            MemoryLayer::Identity,
        ] {
            assert!(SqliteMemory::retention_expiry_for_layer(layer, occurred_at).is_none());
        }
    }

    #[test]
    fn privacy_round_trip() {
        let levels = [
            PrivacyLevel::Public,
            PrivacyLevel::Private,
            PrivacyLevel::Secret,
        ];

        for level in levels {
            let encoded = SqliteMemory::privacy_to_str(&level);
            let decoded = SqliteMemory::str_to_privacy(encoded);
            assert_eq!(decoded, level);
        }
    }

    #[test]
    fn str_to_source_unknown_defaults_to_system() {
        assert_eq!(
            SqliteMemory::str_to_source("unknown-source"),
            MemorySource::System
        );
    }

    #[test]
    fn signal_tier_round_trip_and_default() {
        let tiers = [
            SignalTier::Raw,
            SignalTier::Belief,
            SignalTier::Inferred,
            SignalTier::Governance,
        ];

        for tier in tiers {
            let encoded = SqliteMemory::signal_tier_to_str(tier);
            let decoded = SqliteMemory::str_to_signal_tier(encoded);
            assert_eq!(decoded, tier);
        }

        assert_eq!(
            SqliteMemory::str_to_signal_tier("unknown-tier"),
            SignalTier::Raw
        );
    }

    #[test]
    fn source_kind_round_trip_and_unknown() {
        let kinds = [
            SourceKind::Conversation,
            SourceKind::Discord,
            SourceKind::Telegram,
            SourceKind::Slack,
            SourceKind::Api,
            SourceKind::News,
            SourceKind::Document,
            SourceKind::Manual,
        ];

        for kind in kinds {
            let encoded = SqliteMemory::source_kind_to_str(kind);
            let decoded = SqliteMemory::str_to_source_kind(encoded);
            assert_eq!(decoded, Some(kind));
        }

        assert_eq!(SqliteMemory::str_to_source_kind("unknown-kind"), None);
    }

    #[test]
    fn str_to_privacy_unknown_defaults_to_private() {
        assert_eq!(
            SqliteMemory::str_to_privacy("unknown-privacy"),
            PrivacyLevel::Private
        );
    }

    #[test]
    fn retention_and_layer_strings_stay_in_sync() {
        for layer in all_layers() {
            assert_eq!(
                SqliteMemory::retention_tier_for_layer(layer),
                SqliteMemory::layer_to_str(layer)
            );
        }
    }
}
