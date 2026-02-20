use super::super::traits::{MemoryCategory, MemoryLayer, MemorySource, PrivacyLevel};
use super::LanceDbMemory;

#[allow(
    clippy::unused_self,
    clippy::unused_async,
    clippy::trivially_copy_pass_by_ref
)]
impl LanceDbMemory {
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

    pub(super) fn sql_eq(column: &str, value: &str) -> String {
        let v = value.replace('\'', "''");
        format!("{column} = '{v}'")
    }

    pub(super) fn source_from_category(category: &MemoryCategory) -> MemorySource {
        match category {
            MemoryCategory::Core => MemorySource::ExplicitUser,
            MemoryCategory::Daily => MemorySource::System,
            MemoryCategory::Conversation => MemorySource::Inferred,
            MemoryCategory::Custom(_) => MemorySource::ToolVerified,
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

    pub(super) fn layer_to_str(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    pub(super) fn category_from_source(source: &MemorySource) -> MemoryCategory {
        match source {
            MemorySource::ExplicitUser | MemorySource::ToolVerified => MemoryCategory::Core,
            MemorySource::System => MemoryCategory::Daily,
            MemorySource::Inferred => MemoryCategory::Conversation,
        }
    }
}
