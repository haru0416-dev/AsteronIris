use crate::config::schema::{McpConfig, McpTransport};
use crate::core::tools::Tool;
use crate::plugins::mcp::client_connection::McpConnection;
use crate::plugins::mcp::client_proxy_tool::McpToolProxy;
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
struct ManagedTool {
    connection: Arc<McpConnection>,
    server_name: String,
    tool_name: String,
    description: String,
    parameters_schema: serde_json::Value,
}

pub struct McpManager {
    connections: Vec<Arc<McpConnection>>,
    tools: Vec<ManagedTool>,
}

impl McpManager {
    pub async fn from_config(config: &McpConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self {
                connections: Vec::new(),
                tools: Vec::new(),
            });
        }

        let mut connections: Vec<Arc<McpConnection>> = Vec::new();
        let mut managed_tools = Vec::new();

        for server in config.enabled_servers() {
            if server.max_call_seconds == 0 {
                tracing::warn!(
                    server = %server.name,
                    "Skipping MCP server with invalid max_call_seconds=0"
                );
                continue;
            }

            match &server.transport {
                McpTransport::Stdio { command, args, env } => {
                    if command.is_empty() {
                        tracing::warn!(
                            server = %server.name,
                            "Skipping MCP stdio server with empty command"
                        );
                        continue;
                    }

                    let connection = match McpConnection::connect_stdio(
                        server.name.clone(),
                        command,
                        args,
                        env,
                        server.max_call_seconds,
                    )
                    .await
                    {
                        Ok(connection) => Arc::new(connection),
                        Err(error) => {
                            tracing::warn!(
                                server = %server.name,
                                error = %error,
                                "Failed to connect MCP stdio server"
                            );
                            continue;
                        }
                    };

                    match connection.list_tools().await {
                        Ok(server_tools) => {
                            managed_tools.extend(server_tools.into_iter().map(|tool| {
                                ManagedTool {
                                    connection: Arc::clone(&connection),
                                    server_name: server.name.clone(),
                                    tool_name: tool.name.into_owned(),
                                    description: tool
                                        .description
                                        .map_or_else(String::new, std::borrow::Cow::into_owned),
                                    parameters_schema: serde_json::Value::Object(
                                        tool.input_schema.as_ref().clone(),
                                    ),
                                }
                            }));
                        }
                        Err(error) => {
                            tracing::warn!(
                                server = %server.name,
                                error = %error,
                                "Failed to list MCP tools from server"
                            );
                        }
                    }

                    connections.push(connection);
                }
                McpTransport::Http { .. } => {
                    tracing::warn!(
                        server = %server.name,
                        "MCP HTTP transport is not supported yet; skipping server"
                    );
                }
            }
        }

        Ok(Self {
            connections,
            tools: managed_tools,
        })
    }

    pub fn tools(&self) -> Vec<Box<dyn Tool>> {
        self.tools
            .iter()
            .map(|tool| {
                Box::new(McpToolProxy::new(
                    tool.tool_name.clone(),
                    tool.description.clone(),
                    tool.parameters_schema.clone(),
                    Arc::clone(&tool.connection),
                    tool.server_name.clone(),
                )) as Box<dyn Tool>
            })
            .collect()
    }

    pub async fn shutdown(&self) {
        for connection in &self.connections {
            if let Err(error) = connection.shutdown().await {
                tracing::warn!(
                    server = %connection.name(),
                    error = %error,
                    "Failed to shutdown MCP connection"
                );
            }
        }
    }
}

pub fn create_mcp_tools(config: &McpConfig) -> Vec<Box<dyn Tool>> {
    if !config.enabled {
        return Vec::new();
    }

    let config = config.clone();
    let builder = move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::warn!(error = %error, "Failed to build runtime for MCP tool creation");
                return Vec::new();
            }
        };

        runtime.block_on(async move {
            match McpManager::from_config(&config).await {
                Ok(manager) => manager.tools(),
                Err(error) => {
                    tracing::warn!(error = %error, "Failed to create MCP manager from config");
                    Vec::new()
                }
            }
        })
    };

    if tokio::runtime::Handle::try_current().is_ok() {
        if let Ok(tools) = std::thread::spawn(builder).join() {
            tools
        } else {
            tracing::warn!("MCP tool creation thread panicked");
            Vec::new()
        }
    } else {
        builder()
    }
}
