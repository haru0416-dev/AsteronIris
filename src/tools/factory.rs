use super::{
    ActionOperator, BrowserOpenTool, BrowserTool, ComposioTool, FileReadTool, FileWriteTool,
    MemoryForgetTool, MemoryGovernanceTool, MemoryRecallTool, MemoryStoreTool, NoopOperator,
    ShellTool, Tool,
};
use crate::config::schema::{McpConfig, ToolsConfig};
use crate::memory::Memory;
use crate::security::SecurityPolicy;
use std::sync::Arc;

/// Create the default tool registry
pub fn default_tools(_security: &Arc<SecurityPolicy>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ShellTool::new()),
        Box::new(FileReadTool::new()),
        Box::new(FileWriteTool::new()),
    ]
}

pub fn default_action_operator(security: Arc<SecurityPolicy>) -> Arc<dyn ActionOperator> {
    Arc::new(NoopOperator::new(security))
}

/// Generate tool descriptions for system prompts
///
/// Returns a vector of (`tool_name`, description) tuples.
/// Includes `browser_open` if `browser_enabled` is true.
/// Includes `composio` if `composio_enabled` is true.
pub fn tool_descriptions(
    browser_enabled: bool,
    composio_enabled: bool,
    mcp_config: Option<&McpConfig>,
) -> Vec<(String, String)> {
    let mut descs: Vec<(String, String)> = vec![
        (
            "shell".to_string(),
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.".to_string(),
        ),
        (
            "file_read".to_string(),
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.".to_string(),
        ),
        (
            "file_write".to_string(),
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.".to_string(),
        ),
        (
            "memory_store".to_string(),
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.".to_string(),
        ),
        (
            "memory_recall".to_string(),
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.".to_string(),
        ),
        (
            "memory_forget".to_string(),
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.".to_string(),
        ),
    ];

    if browser_enabled {
        descs.push((
            "browser_open".to_string(),
            "Open approved HTTPS URLs in Brave Browser (allowlist-only, no scraping)".to_string(),
        ));
    }

    if composio_enabled {
        descs.push((
            "composio".to_string(),
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run, 'connect' to OAuth.".to_string(),
        ));
    }

    append_mcp_tool_descriptions(&mut descs, mcp_config);

    descs
}

#[cfg(any(feature = "mcp", test))]
pub(crate) fn append_dynamic_tool_descriptions(
    descriptions: &mut Vec<(String, String)>,
    tools: &[Box<dyn Tool>],
) {
    descriptions.extend(
        tools
            .iter()
            .map(|tool| (tool.name().to_string(), tool.description().to_string())),
    );
}

#[cfg(feature = "mcp")]
pub(crate) fn append_mcp_tool_descriptions(
    descriptions: &mut Vec<(String, String)>,
    mcp_config: Option<&McpConfig>,
) {
    if let Some(config) = mcp_config {
        let mcp_tools = crate::mcp::client::create_mcp_tools(config);
        append_dynamic_tool_descriptions(descriptions, &mcp_tools);
    }
}

#[cfg(not(feature = "mcp"))]
pub(crate) fn append_mcp_tool_descriptions(
    _descriptions: &mut Vec<(String, String)>,
    _mcp_config: Option<&McpConfig>,
) {
}

/// Create full tool registry including memory tools and optional Composio
pub fn all_tools(
    security: &Arc<SecurityPolicy>,
    memory: Arc<dyn Memory>,
    composio_key: Option<&str>,
    browser_config: &crate::config::BrowserConfig,
    tools_config: &ToolsConfig,
    mcp_config: Option<&McpConfig>,
) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    if tools_config.shell.enabled {
        tools.push(Box::new(ShellTool::new()));
    }

    if tools_config.file_read.enabled {
        tools.push(Box::new(FileReadTool::new()));
    }

    if tools_config.file_write.enabled {
        tools.push(Box::new(FileWriteTool::new()));
    }

    if tools_config.memory_store.enabled {
        tools.push(Box::new(MemoryStoreTool::new(Arc::clone(&memory))));
    }

    if tools_config.memory_recall.enabled {
        tools.push(Box::new(MemoryRecallTool::new(Arc::clone(&memory))));
    }

    if tools_config.memory_forget.enabled {
        tools.push(Box::new(MemoryForgetTool::new(Arc::clone(&memory))));
    }

    if tools_config.memory_governance.enabled {
        tools.push(Box::new(MemoryGovernanceTool::new(memory)));
    }

    if browser_config.enabled {
        // Add legacy browser_open tool for simple URL opening
        tools.push(Box::new(BrowserOpenTool::new(
            browser_config.allowed_domains.clone(),
        )));
        // Add full browser automation tool (agent-browser)
        tools.push(Box::new(BrowserTool::new(
            Arc::clone(security),
            browser_config.allowed_domains.clone(),
            browser_config.session_name.clone(),
        )));
    }

    if let Some(key) = composio_key
        && !key.is_empty()
    {
        tools.push(Box::new(ComposioTool::new(key)));
    }

    append_mcp_tools(&mut tools, mcp_config);

    tools
}

#[cfg(feature = "mcp")]
pub(crate) fn append_mcp_tools(tools: &mut Vec<Box<dyn Tool>>, mcp_config: Option<&McpConfig>) {
    if let Some(config) = mcp_config {
        tools.extend(crate::mcp::client::create_mcp_tools(config));
    }
}

#[cfg(not(feature = "mcp"))]
pub(crate) fn append_mcp_tools(_tools: &mut Vec<Box<dyn Tool>>, _mcp_config: Option<&McpConfig>) {}
