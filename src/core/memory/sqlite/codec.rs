use super::SqliteMemory;
use crate::core::memory::traits::MemoryLayer;
use crate::core::memory::{MemoryCategory, MemorySource, PrivacyLevel};

impl SqliteMemory {
    // Used by projection layer methods (upsert/list_projection_entry) — currently dormant
    pub(super) fn category_to_str(cat: &MemoryCategory) -> String {
        match cat {
            MemoryCategory::Core => "core".into(),
            MemoryCategory::Daily => "daily".into(),
            MemoryCategory::Conversation => "conversation".into(),
            MemoryCategory::Custom(name) => name.clone(),
        }
    }

    // Used by projection layer methods (fetch/list/keyword search) — currently dormant
    pub(super) fn str_to_category(s: &str) -> MemoryCategory {
        match s {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            other => MemoryCategory::custom(other),
        }
    }

    pub(super) fn source_to_str(source: MemorySource) -> &'static str {
        match source {
            MemorySource::ExplicitUser => "explicit_user",
            MemorySource::ToolVerified => "tool_verified",
            MemorySource::System => "system",
            MemorySource::Inferred => "inferred",
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
    fn category_round_trip() {
        let categories = [
            MemoryCategory::Core,
            MemoryCategory::Daily,
            MemoryCategory::Conversation,
            MemoryCategory::Custom("foo".to_string()),
        ];

        for category in categories {
            let encoded = SqliteMemory::category_to_str(&category);
            let decoded = SqliteMemory::str_to_category(&encoded);
            assert_eq!(decoded, category);
        }
    }

    #[test]
    fn source_round_trip_for_all_variants() {
        let sources = [
            MemorySource::ExplicitUser,
            MemorySource::ToolVerified,
            MemorySource::System,
            MemorySource::Inferred,
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
