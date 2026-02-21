use crate::plugins::mcp::bridge::from_rmcp_contents;
use crate::plugins::mcp::content::ToolContent;
use anyhow::{Context, Result, anyhow};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::RwLock;

type McpService = RunningService<RoleClient, ()>;

pub struct McpConnection {
    name: String,
    service: Arc<RwLock<Option<McpService>>>,
    max_call_seconds: u64,
}

impl McpConnection {
    pub async fn connect_stdio(
        name: impl Into<String>,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        max_call_seconds: u64,
    ) -> Result<Self> {
        let service = ()
            .serve(TokioChildProcess::new(Command::new(command).configure(
                |cmd| {
                    cmd.args(args);
                    cmd.envs(env.iter());
                },
            ))?)
            .await
            .with_context(|| format!("failed to connect MCP server '{command}' over stdio"))?;

        Ok(Self {
            name: name.into(),
            service: Arc::new(RwLock::new(Some(service))),
            max_call_seconds,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn list_tools(&self) -> Result<Vec<rmcp::model::Tool>> {
        let service_guard = self.service.read().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| anyhow!("MCP connection '{}' is not active", self.name))?;

        let tools = service
            .list_all_tools()
            .await
            .with_context(|| format!("failed to list tools for MCP server '{}'", self.name))?;
        Ok(tools)
    }

    pub async fn call_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<Vec<ToolContent>> {
        let arguments = match args {
            serde_json::Value::Object(object) => Some(object),
            serde_json::Value::Null => None,
            _ => {
                return Err(anyhow!(
                    "MCP tool '{tool_name}' requires JSON object arguments"
                ));
            }
        };

        let request = CallToolRequestParams {
            meta: None,
            name: tool_name.to_string().into(),
            arguments,
            task: None,
        };

        let service_guard = self.service.read().await;
        let service = service_guard
            .as_ref()
            .ok_or_else(|| anyhow!("MCP connection '{}' is not active", self.name))?;

        let result = tokio::time::timeout(
            Duration::from_secs(self.max_call_seconds),
            service.call_tool(request),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "MCP tool '{}' on server '{}' timed out after {}s",
                tool_name,
                self.name,
                self.max_call_seconds
            )
        })?
        .with_context(|| {
            format!(
                "MCP tool '{}' call failed on server '{}'",
                tool_name, self.name
            )
        })?;

        Ok(from_rmcp_contents(&result.content))
    }

    pub async fn shutdown(&self) -> Result<()> {
        let service = self.service.write().await.take();
        if let Some(service) = service {
            service
                .cancel()
                .await
                .with_context(|| format!("failed to shutdown MCP server '{}'", self.name))?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn disconnected_for_test(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            service: Arc::new(RwLock::new(None)),
            max_call_seconds: 30,
        }
    }
}
