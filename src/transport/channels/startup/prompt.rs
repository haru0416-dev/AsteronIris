use crate::config::Config;

use super::super::prompt_builder::build_system_prompt;

pub(super) fn build_channel_system_prompt(
    config: &Config,
    workspace: &std::path::Path,
    model: &str,
    skills: &[crate::plugins::skills::Skill],
) -> String {
    let tool_descs = crate::core::tools::tool_descriptions(
        config.browser.enabled,
        config.composio.enabled,
        Some(&config.mcp),
    );
    let prompt_tool_descs: Vec<(&str, &str)> = tool_descs
        .iter()
        .map(|(name, description)| (name.as_str(), description.as_str()))
        .collect();
    build_system_prompt(workspace, model, &prompt_tool_descs, skills)
}
