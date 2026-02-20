pub mod browser;
pub mod browser_open;
pub mod composio;
pub mod file_read;
pub mod file_write;
pub mod memory_forget;
pub mod memory_governance;
pub mod memory_recall;
pub mod memory_store;
pub mod middleware;
pub mod registry;
pub mod shell;
pub mod traits;

pub use browser::BrowserTool;
pub use browser_open::BrowserOpenTool;
pub use composio::ComposioTool;
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use memory_forget::MemoryForgetTool;
pub use memory_governance::MemoryGovernanceTool;
pub use memory_recall::MemoryRecallTool;
pub use memory_store::MemoryStoreTool;
#[allow(unused_imports)]
pub use middleware::{
    ExecutionContext, MiddlewareDecision, ToolMiddleware, default_middleware_chain,
};
#[allow(unused_imports)]
pub use registry::ToolRegistry;
pub use shell::ShellTool;
pub use traits::Tool;
#[allow(unused_imports)]
pub use traits::{ActionIntent, ActionOperator, ActionResult, NoopOperator, ToolResult, ToolSpec};

use crate::config::schema::ToolsConfig;
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
) -> Vec<(&'static str, &'static str)> {
    let mut descs: Vec<(&str, &str)> = vec![
        (
            "shell",
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
        ),
        (
            "file_read",
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
        ),
        (
            "file_write",
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
        ),
        (
            "memory_store",
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
        ),
        (
            "memory_recall",
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
        ),
        (
            "memory_forget",
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
        ),
    ];

    if browser_enabled {
        descs.push((
            "browser_open",
            "Open approved HTTPS URLs in Brave Browser (allowlist-only, no scraping)",
        ));
    }

    if composio_enabled {
        descs.push((
            "composio",
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run, 'connect' to OAuth.",
        ));
    }

    descs
}

/// Create full tool registry including memory tools and optional Composio
pub fn all_tools(
    security: &Arc<SecurityPolicy>,
    memory: Arc<dyn Memory>,
    composio_key: Option<&str>,
    browser_config: &crate::config::BrowserConfig,
    tools_config: &ToolsConfig,
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

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::ToolEntry;
    use crate::config::{BrowserConfig, MemoryConfig};
    use tempfile::TempDir;

    #[test]
    fn default_tools_has_three() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(&security);
        assert_eq!(tools.len(), 3);
    }

    #[test]
    fn all_tools_excludes_browser_when_disabled() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig {
            enabled: false,
            allowed_domains: vec!["example.com".into()],
            session_name: None,
        };

        let tools_cfg = ToolsConfig::default();
        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"browser_open"));
    }

    #[test]
    fn all_tools_includes_browser_when_enabled() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig {
            enabled: true,
            allowed_domains: vec!["example.com".into()],
            session_name: None,
        };

        let tools_cfg = ToolsConfig::default();
        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"browser_open"));
    }

    #[test]
    fn default_tools_names() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(&security);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
    }

    #[test]
    fn default_tools_all_have_descriptions() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(&security);
        for tool in &tools {
            assert!(
                !tool.description().is_empty(),
                "Tool {} has empty description",
                tool.name()
            );
        }
    }

    #[test]
    fn default_tools_all_have_schemas() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(&security);
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "Tool {} schema is not an object",
                tool.name()
            );
            assert!(
                schema["properties"].is_object(),
                "Tool {} schema has no properties",
                tool.name()
            );
        }
    }

    #[test]
    fn tool_spec_generation() {
        let security = Arc::new(SecurityPolicy::default());
        let tools = default_tools(&security);
        for tool in &tools {
            let spec = tool.spec();
            assert_eq!(spec.name, tool.name());
            assert_eq!(spec.description, tool.description());
            assert!(spec.parameters.is_object());
        }
    }

    #[test]
    fn tool_result_serde() {
        let result = ToolResult {
            success: true,
            output: "hello".into(),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.output, "hello");
        assert!(parsed.error.is_none());
    }

    #[test]
    fn tool_result_with_error_serde() {
        let result = ToolResult {
            success: false,
            output: String::new(),
            error: Some("boom".into()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(!parsed.success);
        assert_eq!(parsed.error.as_deref(), Some("boom"));
    }

    #[test]
    fn tool_spec_serde() {
        let spec = ToolSpec {
            name: "test".into(),
            description: "A test tool".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&spec).unwrap();
        let parsed: ToolSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test");
        assert_eq!(parsed.description, "A test tool");
    }

    #[test]
    fn all_tools_respects_disabled_shell() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let mut tools_cfg = ToolsConfig::default();
        tools_cfg.shell.enabled = false;

        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"shell"));
    }

    #[test]
    fn all_tools_respects_disabled_file_read() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let mut tools_cfg = ToolsConfig::default();
        tools_cfg.file_read.enabled = false;

        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"file_read"));
    }

    #[test]
    fn all_tools_respects_disabled_memory_forget() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let tools_cfg = ToolsConfig::default();

        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"memory_forget"));
    }

    #[test]
    fn all_tools_includes_memory_forget_when_enabled() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let mut tools_cfg = ToolsConfig::default();
        tools_cfg.memory_forget.enabled = true;

        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"memory_forget"));
    }

    #[test]
    fn all_tools_with_all_disabled_yields_empty() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let tools_cfg = ToolsConfig {
            shell: ToolEntry { enabled: false },
            file_read: ToolEntry { enabled: false },
            file_write: ToolEntry { enabled: false },
            memory_store: ToolEntry { enabled: false },
            memory_recall: ToolEntry { enabled: false },
            memory_forget: ToolEntry { enabled: false },
            memory_governance: ToolEntry { enabled: false },
        };

        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        assert_eq!(tools.len(), 0);
    }

    #[test]
    fn all_tools_default_config_has_expected_tools() {
        let tmp = TempDir::new().unwrap();
        let security = Arc::new(SecurityPolicy::default());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(crate::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let browser = BrowserConfig::default();
        let tools_cfg = ToolsConfig::default();

        let tools = all_tools(&security, mem, None, &browser, &tools_cfg);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

        assert!(names.contains(&"shell"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"memory_store"));
        assert!(names.contains(&"memory_recall"));
        assert!(!names.contains(&"memory_forget"));
        assert!(!names.contains(&"memory_governance"));
    }
}
