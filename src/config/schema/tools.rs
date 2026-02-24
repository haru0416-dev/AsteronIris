use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_tool_enabled() -> ToolEntry {
    ToolEntry { enabled: true }
}

fn default_tool_disabled() -> ToolEntry {
    ToolEntry { enabled: false }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    #[serde(default = "default_tool_enabled")]
    pub shell: ToolEntry,
    #[serde(default = "default_tool_enabled")]
    pub file_read: ToolEntry,
    #[serde(default = "default_tool_enabled")]
    pub file_write: ToolEntry,
    #[serde(default = "default_tool_enabled")]
    pub memory_store: ToolEntry,
    #[serde(default = "default_tool_enabled")]
    pub memory_recall: ToolEntry,
    #[serde(default = "default_tool_disabled")]
    pub memory_forget: ToolEntry,
    #[serde(default = "default_tool_disabled")]
    pub memory_governance: ToolEntry,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            shell: ToolEntry { enabled: true },
            file_read: ToolEntry { enabled: true },
            file_write: ToolEntry { enabled: true },
            memory_store: ToolEntry { enabled: true },
            memory_recall: ToolEntry { enabled: true },
            memory_forget: ToolEntry { enabled: false },
            memory_governance: ToolEntry { enabled: false },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tools_config_has_correct_enabled_flags() {
        let cfg = ToolsConfig::default();
        assert!(cfg.shell.enabled);
        assert!(cfg.file_read.enabled);
        assert!(cfg.file_write.enabled);
        assert!(cfg.memory_store.enabled);
        assert!(cfg.memory_recall.enabled);
        assert!(!cfg.memory_forget.enabled);
        assert!(!cfg.memory_governance.enabled);
    }

    #[test]
    fn tools_config_deserialize_with_defaults() {
        let toml_str = "[tools]";
        let cfg: ToolsConfig = toml::from_str(toml_str).unwrap();
        assert!(cfg.shell.enabled);
    }

    #[test]
    fn tools_config_deserialize_with_overrides() {
        let toml_str = r#"
shell = { enabled = false }
memory_forget = { enabled = true }
"#;
        let cfg: ToolsConfig = toml::from_str(toml_str).unwrap();
        assert!(!cfg.shell.enabled);
        assert!(cfg.memory_forget.enabled);
    }
}
