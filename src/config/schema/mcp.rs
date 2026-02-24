use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

fn default_max_call_seconds() -> u64 {
    30
}
fn default_enabled_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_json: Option<String>,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default)]
    pub transport: McpTransport,
    #[serde(default = "default_enabled_true")]
    pub enabled: bool,
    #[serde(default = "default_max_call_seconds")]
    pub max_call_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Http {
        url: String,
    },
}

impl Default for McpTransport {
    fn default() -> Self {
        Self::Stdio {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
        }
    }
}

impl McpConfig {
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut names: HashSet<&str> = HashSet::new();

        for server in &self.servers {
            if server.name.is_empty() {
                errors.push("MCP server name cannot be empty".to_string());
            }
            if !names.insert(server.name.as_str()) {
                errors.push(format!("Duplicate MCP server name: {}", server.name));
            }
            match &server.transport {
                McpTransport::Stdio { command, .. } => {
                    if command.is_empty() {
                        errors.push(format!(
                            "MCP server '{}': stdio transport requires a command",
                            server.name
                        ));
                    }
                }
                McpTransport::Http { url } => {
                    if url.is_empty() {
                        errors.push(format!(
                            "MCP server '{}': http transport requires a url",
                            server.name
                        ));
                    }
                    match crate::security::url_validation::validate_url_not_ssrf(url) {
                        Ok(()) => {}
                        Err(_) if url::Url::parse(url).is_err() => {
                            errors.push(format!(
                                "MCP server '{}': http transport has an invalid URL",
                                server.name
                            ));
                        }
                        Err(_) => {
                            errors.push(format!(
                                "MCP server '{}': http URL points to private/internal address",
                                server.name
                            ));
                        }
                    }
                }
            }
            if server.max_call_seconds == 0 {
                errors.push(format!(
                    "MCP server '{}': max_call_seconds must be > 0",
                    server.name
                ));
            }
        }

        errors
    }

    #[must_use]
    pub fn enabled_servers(&self) -> Vec<&McpServerConfig> {
        self.servers
            .iter()
            .filter(|server| server.enabled)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_disabled_with_no_servers() {
        let cfg = McpConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.import_json.is_none());
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn validate_rejects_empty_name() {
        let cfg = McpConfig {
            enabled: true,
            import_json: None,
            servers: vec![McpServerConfig {
                name: String::new(),
                transport: McpTransport::Stdio {
                    command: "npx".to_string(),
                    args: vec![],
                    env: HashMap::new(),
                },
                enabled: true,
                max_call_seconds: 30,
            }],
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|error| error.contains("cannot be empty")));
    }

    #[test]
    fn validate_rejects_duplicate_names() {
        let server = McpServerConfig {
            name: "test".to_string(),
            transport: McpTransport::Stdio {
                command: "echo".to_string(),
                args: vec![],
                env: HashMap::new(),
            },
            enabled: true,
            max_call_seconds: 30,
        };
        let cfg = McpConfig {
            enabled: true,
            import_json: None,
            servers: vec![server.clone(), server],
        };
        let errors = cfg.validate();
        assert!(errors.iter().any(|error| error.contains("Duplicate")));
    }

    #[test]
    fn enabled_servers_filters_disabled() {
        let cfg = McpConfig {
            enabled: true,
            import_json: None,
            servers: vec![
                McpServerConfig {
                    name: "on".to_string(),
                    transport: McpTransport::Stdio {
                        command: "echo".to_string(),
                        args: vec![],
                        env: HashMap::new(),
                    },
                    enabled: true,
                    max_call_seconds: 30,
                },
                McpServerConfig {
                    name: "off".to_string(),
                    transport: McpTransport::Stdio {
                        command: "echo".to_string(),
                        args: vec![],
                        env: HashMap::new(),
                    },
                    enabled: false,
                    max_call_seconds: 30,
                },
            ],
        };
        let enabled = cfg.enabled_servers();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "on");
    }
}
