use super::types::ToolResult;
use serde_json::json;

#[cfg(test)]
use crate::security::{AutonomyLevel, SecurityPolicy};
#[cfg(test)]
use std::sync::Arc;

pub(crate) fn workspace_path_property() -> serde_json::Value {
    json!({
        "type": "string",
        "description": "Relative path to the file within the workspace"
    })
}

pub(crate) fn failed_tool_result(message: impl Into<String>) -> ToolResult {
    ToolResult {
        success: false,
        output: String::new(),
        error: Some(message.into()),
        attachments: Vec::new(),
    }
}

#[cfg(test)]
pub(crate) fn test_security_policy(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::Supervised,
        workspace_dir: workspace,
        ..SecurityPolicy::default()
    })
}
