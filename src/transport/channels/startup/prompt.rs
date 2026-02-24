use crate::config::Config;

use super::super::prompt_builder::build_system_prompt;

pub(super) fn build_channel_system_prompt(
    _config: &Config,
    workspace: &std::path::Path,
    model: &str,
) -> String {
    let tool_descs = crate::tools::tool_descriptions();
    let prompt_tool_descs: Vec<(&str, &str)> = tool_descs
        .iter()
        .map(|(name, description)| (name.as_str(), description.as_str()))
        .collect();
    build_system_prompt(workspace, model, &prompt_tool_descs)
}
