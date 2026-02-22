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
            other => MemoryCategory::custom(other),
        }
    }

    fn sanitize_sql_value(value: &str) -> String {
        value
            .chars()
            .filter(|c| !c.is_control() && *c != '\\' && *c != '\0')
            .take(256)
            .collect::<String>()
            .replace('\'', "''")
    }

    pub(super) fn sql_eq(column: &str, value: &str) -> String {
        let v = Self::sanitize_sql_value(value);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_eq_basic() {
        let result = LanceDbMemory::sql_eq("col", "hello");
        assert_eq!(result, "col = 'hello'");
    }

    #[test]
    fn sql_eq_escapes_single_quotes() {
        let result = LanceDbMemory::sql_eq("col", "it's");
        assert_eq!(result, "col = 'it''s'");
    }

    #[test]
    fn sql_eq_strips_backslash() {
        let result = LanceDbMemory::sql_eq("col", "hello\\world");
        assert!(!result.contains('\\'), "Backslash should be stripped");
    }

    #[test]
    fn sql_eq_strips_null_bytes() {
        let result = LanceDbMemory::sql_eq("col", "hello\0world");
        assert!(!result.contains('\0'), "Null byte should be stripped");
    }

    #[test]
    fn sql_eq_caps_length() {
        let long_input = "a".repeat(500);
        let result = LanceDbMemory::sql_eq("col", &long_input);
        // Extract the value part between quotes
        let value_part = result
            .strip_prefix("col = '")
            .unwrap_or("")
            .strip_suffix('\'')
            .unwrap_or("");
        assert!(
            value_part.len() <= 256,
            "Value should be capped at 256 chars, got {}",
            value_part.len()
        );
    }

    #[test]
    fn sql_eq_strips_control_chars() {
        let input = "hello\x01world\x02test";
        let result = LanceDbMemory::sql_eq("col", input);
        assert!(!result.contains('\x01'), "Control char should be stripped");
        assert!(!result.contains('\x02'), "Control char should be stripped");
    }

    #[test]
    fn str_to_category_uses_custom_constructor() {
        let result = LanceDbMemory::str_to_category("my_custom_category");
        match result {
            MemoryCategory::Custom(name) => {
                // Should be sanitized by the custom() constructor
                assert_eq!(name, "my_custom_category");
            }
            _ => panic!("Expected Custom variant"),
        }
    }
}
