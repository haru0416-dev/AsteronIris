use super::*;
use crate::config::schema::{McpConfig, McpServerConfig, McpTransport, ToolEntry, ToolsConfig};
use crate::config::{BrowserConfig, MemoryConfig};
use crate::core::memory::Memory;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;

fn markdown_memory(tmp: &TempDir) -> Arc<dyn Memory> {
    let mem_cfg = MemoryConfig {
        backend: "markdown".into(),
        ..MemoryConfig::default()
    };
    Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap())
}

fn enabled_mcp_config_without_servers() -> McpConfig {
    McpConfig {
        enabled: true,
        import_json: None,
        servers: Vec::new(),
    }
}

#[cfg(not(feature = "mcp"))]
fn enabled_mcp_config_with_empty_server() -> McpConfig {
    McpConfig {
        enabled: true,
        import_json: None,
        servers: vec![McpServerConfig {
            name: "empty".to_string(),
            transport: McpTransport::Stdio {
                command: String::new(),
                args: Vec::new(),
                env: HashMap::new(),
            },
            enabled: true,
            max_call_seconds: 30,
        }],
    }
}

struct MockTool {
    name: String,
    description: String,
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            success: true,
            output: String::new(),
            error: None,

            attachments: Vec::new(),
        })
    }
}

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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig {
        enabled: false,
        allowed_domains: vec!["example.com".into()],
        session_name: None,
    };

    let tools_cfg = ToolsConfig::default();
    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig {
        enabled: true,
        allowed_domains: vec!["example.com".into()],
        session_name: None,
    };

    let tools_cfg = ToolsConfig::default();
    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
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
        attachments: Vec::new(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: ToolResult = serde_json::from_str(&json).unwrap();
    assert!(parsed.success);
    assert_eq!(parsed.output, "hello");
    assert!(parsed.error.is_none());
    assert!(parsed.attachments.is_empty());
}

#[test]
fn tool_result_with_error_serde() {
    let result = ToolResult {
        success: false,
        output: String::new(),
        error: Some("boom".into()),
        attachments: Vec::new(),
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: ToolResult = serde_json::from_str(&json).unwrap();
    assert!(!parsed.success);
    assert_eq!(parsed.error.as_deref(), Some("boom"));
    assert!(parsed.attachments.is_empty());
}

#[test]
fn tool_result_deserialize_without_attachments_still_works() {
    let json = r#"{"success":true,"output":"ok","error":null}"#;
    let parsed: ToolResult = serde_json::from_str(json).unwrap();
    assert!(parsed.success);
    assert!(parsed.attachments.is_empty());
}

#[test]
fn tool_result_with_attachments_serde() {
    let result = ToolResult {
        success: true,
        output: "image generated".into(),
        error: None,
        attachments: vec![OutputAttachment::from_path(
            "image/png",
            "/tmp/generated.png",
            Some("generated.png".to_string()),
        )],
    };
    let json = serde_json::to_string(&result).unwrap();
    let parsed: ToolResult = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.attachments.len(), 1);
    assert_eq!(
        parsed.attachments[0].path.as_deref(),
        Some("/tmp/generated.png")
    );
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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig::default();
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.shell.enabled = false;

    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig::default();
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.file_read.enabled = false;

    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig::default();
    let tools_cfg = ToolsConfig::default();

    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig::default();
    let mut tools_cfg = ToolsConfig::default();
    tools_cfg.memory_forget.enabled = true;

    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"memory_forget"));
}

#[test]
fn all_tools_with_all_disabled_yields_only_always_on_tools() {
    let tmp = TempDir::new().unwrap();
    let security = Arc::new(SecurityPolicy::default());
    let mem_cfg = MemoryConfig {
        backend: "markdown".into(),
        ..MemoryConfig::default()
    };
    let mem: Arc<dyn Memory> =
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

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
    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
    // Always-on tools: delegate, subagent_spawn, subagent_output, subagent_cancel
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(!names.contains(&"shell"));
    assert!(!names.contains(&"file_read"));
    assert!(!names.contains(&"memory_store"));
    assert!(names.contains(&"delegate"));
    assert_eq!(
        tools.len(),
        4,
        "only always-on tools should remain: {names:?}"
    );
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
        Arc::from(crate::core::memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

    let browser = BrowserConfig::default();
    let tools_cfg = ToolsConfig::default();

    let tools = all_tools(&security, mem, None, &browser, &tools_cfg, None);
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

    assert!(names.contains(&"shell"));
    assert!(names.contains(&"file_read"));
    assert!(names.contains(&"file_write"));
    assert!(names.contains(&"memory_store"));
    assert!(names.contains(&"memory_recall"));
    assert!(!names.contains(&"memory_forget"));
    assert!(!names.contains(&"memory_governance"));
}

#[test]
fn tool_descriptions_contains_core_tools() {
    let descriptions = tool_descriptions(false, false, None);
    let names: Vec<&str> = descriptions
        .iter()
        .map(|(name, _description)| name.as_str())
        .collect();
    assert!(names.contains(&"shell"));
    assert!(names.contains(&"file_read"));
    assert!(names.contains(&"file_write"));
    assert!(names.contains(&"memory_store"));
    assert!(names.contains(&"memory_recall"));
    assert!(names.contains(&"memory_forget"));
}

#[test]
fn tool_descriptions_respects_browser_flag() {
    let disabled = tool_descriptions(false, false, None);
    let enabled = tool_descriptions(true, false, None);
    let disabled_names: Vec<&str> = disabled
        .iter()
        .map(|(name, _description)| name.as_str())
        .collect();
    let enabled_names: Vec<&str> = enabled
        .iter()
        .map(|(name, _description)| name.as_str())
        .collect();
    assert!(!disabled_names.contains(&"browser_open"));
    assert!(enabled_names.contains(&"browser_open"));
}

#[test]
fn tool_descriptions_respects_composio_flag() {
    let disabled = tool_descriptions(false, false, None);
    let enabled = tool_descriptions(false, true, None);
    let disabled_names: Vec<&str> = disabled
        .iter()
        .map(|(name, _description)| name.as_str())
        .collect();
    let enabled_names: Vec<&str> = enabled
        .iter()
        .map(|(name, _description)| name.as_str())
        .collect();
    assert!(!disabled_names.contains(&"composio"));
    assert!(enabled_names.contains(&"composio"));
}

#[test]
fn all_tools_none_mcp_matches_empty_mcp_config() {
    let tmp = TempDir::new().unwrap();
    let security = Arc::new(SecurityPolicy::default());
    let browser = BrowserConfig::default();
    let tools_cfg = ToolsConfig::default();
    let baseline = all_tools(
        &security,
        markdown_memory(&tmp),
        None,
        &browser,
        &tools_cfg,
        None,
    );
    let with_empty_config = all_tools(
        &security,
        markdown_memory(&tmp),
        None,
        &browser,
        &tools_cfg,
        Some(&enabled_mcp_config_without_servers()),
    );

    let baseline_names: Vec<&str> = baseline.iter().map(|tool| tool.name()).collect();
    let empty_names: Vec<&str> = with_empty_config.iter().map(|tool| tool.name()).collect();
    assert_eq!(baseline_names, empty_names);
}

#[test]
fn tool_descriptions_none_mcp_matches_empty_mcp_config() {
    let baseline = tool_descriptions(false, false, None);
    let with_empty_config =
        tool_descriptions(false, false, Some(&enabled_mcp_config_without_servers()));
    assert_eq!(baseline, with_empty_config);
}

#[test]
fn append_dynamic_tool_descriptions_keeps_namespaced_mcp_names() {
    let mut descriptions = vec![("shell".to_string(), "run commands".to_string())];
    let dynamic_tools: Vec<Box<dyn Tool>> = vec![
        Box::new(MockTool {
            name: "mcp_filesystem_search".to_string(),
            description: "Search files".to_string(),
        }),
        Box::new(MockTool {
            name: "mcp_github_get_issue".to_string(),
            description: "Fetch issue".to_string(),
        }),
    ];

    append_dynamic_tool_descriptions(&mut descriptions, &dynamic_tools);
    let dynamic_names: Vec<&str> = descriptions
        .iter()
        .skip(1)
        .map(|(name, _description)| name.as_str())
        .collect();
    assert!(dynamic_names.iter().all(|name| name.starts_with("mcp_")));
}

#[test]
fn append_dynamic_tool_descriptions_appends_tool_descriptions() {
    let mut descriptions = vec![("shell".to_string(), "run commands".to_string())];
    let dynamic_tools: Vec<Box<dyn Tool>> = vec![Box::new(MockTool {
        name: "mcp_docs_lookup".to_string(),
        description: "Lookup docs".to_string(),
    })];

    append_dynamic_tool_descriptions(&mut descriptions, &dynamic_tools);

    assert_eq!(descriptions.len(), 2);
    assert_eq!(descriptions[1].0, "mcp_docs_lookup");
    assert_eq!(descriptions[1].1, "Lookup docs");
}

#[cfg(not(feature = "mcp"))]
#[test]
fn all_tools_accepts_mcp_config_but_ignores_it_without_feature() {
    let tmp = TempDir::new().unwrap();
    let security = Arc::new(SecurityPolicy::default());
    let browser = BrowserConfig::default();
    let tools_cfg = ToolsConfig::default();
    let with_none = all_tools(
        &security,
        markdown_memory(&tmp),
        None,
        &browser,
        &tools_cfg,
        None,
    );
    let with_enabled_mcp = all_tools(
        &security,
        markdown_memory(&tmp),
        None,
        &browser,
        &tools_cfg,
        Some(&enabled_mcp_config_with_empty_server()),
    );

    let none_names: Vec<&str> = with_none.iter().map(|tool| tool.name()).collect();
    let enabled_names: Vec<&str> = with_enabled_mcp.iter().map(|tool| tool.name()).collect();
    assert_eq!(none_names, enabled_names);
}

#[cfg(not(feature = "mcp"))]
#[test]
fn tool_descriptions_accepts_mcp_config_but_ignores_it_without_feature() {
    let with_none = tool_descriptions(false, false, None);
    let with_enabled_mcp =
        tool_descriptions(false, false, Some(&enabled_mcp_config_with_empty_server()));
    assert_eq!(with_none, with_enabled_mcp);
}

#[cfg(feature = "mcp")]
#[test]
fn all_tools_with_empty_mcp_servers_matches_none() {
    let tmp = TempDir::new().unwrap();
    let security = Arc::new(SecurityPolicy::default());
    let browser = BrowserConfig::default();
    let tools_cfg = ToolsConfig::default();
    let with_none = all_tools(
        &security,
        markdown_memory(&tmp),
        None,
        &browser,
        &tools_cfg,
        None,
    );
    let with_empty_servers = all_tools(
        &security,
        markdown_memory(&tmp),
        None,
        &browser,
        &tools_cfg,
        Some(&enabled_mcp_config_without_servers()),
    );

    let none_names: Vec<&str> = with_none.iter().map(|tool| tool.name()).collect();
    let empty_names: Vec<&str> = with_empty_servers.iter().map(|tool| tool.name()).collect();
    assert_eq!(none_names, empty_names);
}

#[cfg(feature = "mcp")]
#[test]
fn tool_descriptions_with_empty_mcp_servers_matches_none() {
    let with_none = tool_descriptions(false, false, None);
    let with_empty_servers =
        tool_descriptions(false, false, Some(&enabled_mcp_config_without_servers()));
    assert_eq!(with_none, with_empty_servers);
}

#[cfg(feature = "mcp")]
#[test]
fn tool_descriptions_with_disabled_mcp_config_matches_none() {
    let with_none = tool_descriptions(false, false, None);
    let disabled_mcp = McpConfig::default();
    let with_disabled = tool_descriptions(false, false, Some(&disabled_mcp));
    assert_eq!(with_none, with_disabled);
}
