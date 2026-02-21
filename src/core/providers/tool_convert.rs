use crate::core::tools::traits::ToolSpec;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolFields {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolFields {
    #[must_use]
    pub fn from_tool(tool: &ToolSpec) -> Self {
        Self {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        }
    }

    #[must_use]
    pub fn from_tool_with_description(tool: &ToolSpec, description: String) -> Self {
        Self {
            name: tool.name.clone(),
            description,
            parameters: tool.parameters.clone(),
        }
    }
}

pub fn map_tools_optional<T, F>(tools: &[ToolSpec], mut map: F) -> Option<Vec<T>>
where
    F: FnMut(&ToolSpec) -> T,
{
    if tools.is_empty() {
        None
    } else {
        Some(tools.iter().map(&mut map).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolFields, map_tools_optional};
    use crate::core::tools::traits::ToolSpec;

    #[test]
    fn map_tools_optional_returns_none_for_empty_slice() {
        let mapped: Option<Vec<String>> = map_tools_optional(&[], |tool| tool.name.clone());
        assert!(mapped.is_none());
    }

    #[test]
    fn map_tools_optional_maps_each_tool() {
        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Run command".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let mapped = map_tools_optional(&tools, |tool| tool.name.clone());
        assert_eq!(mapped, Some(vec!["shell".to_string()]));
    }

    #[test]
    fn tool_fields_clone_name_description_and_parameters() {
        let tool = ToolSpec {
            name: "shell".to_string(),
            description: "Run command".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };

        let fields = ToolFields::from_tool(&tool);
        assert_eq!(fields.name, "shell");
        assert_eq!(fields.description, "Run command");
        assert_eq!(fields.parameters["type"], "object");
    }

    #[test]
    fn tool_fields_support_custom_description() {
        let tool = ToolSpec {
            name: "shell".to_string(),
            description: "Run command".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        };

        let fields = ToolFields::from_tool_with_description(&tool, "Redacted".to_string());
        assert_eq!(fields.description, "Redacted");
        assert_eq!(fields.name, "shell");
    }
}
