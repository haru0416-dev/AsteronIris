use super::SqliteMemory;
use crate::memory::traits::MemoryLayer;
use crate::memory::{MemoryCategory, MemorySource, PrivacyLevel};

impl SqliteMemory {
    pub(super) fn category_to_str(cat: &MemoryCategory) -> String {
        match cat {
            MemoryCategory::Core => "core".into(),
            MemoryCategory::Daily => "daily".into(),
            MemoryCategory::Conversation => "conversation".into(),
            MemoryCategory::Custom(name) => name.clone(),
        }
    }

    pub(super) fn str_to_category(s: &str) -> MemoryCategory {
        match s {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }

    pub(super) fn source_to_str(source: &MemorySource) -> &'static str {
        match source {
            MemorySource::ExplicitUser => "explicit_user",
            MemorySource::ToolVerified => "tool_verified",
            MemorySource::System => "system",
            MemorySource::Inferred => "inferred",
        }
    }

    pub(super) fn layer_to_str(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    pub(super) fn retention_tier_for_layer(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    pub(super) fn retention_expiry_for_layer(
        layer: &MemoryLayer,
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
