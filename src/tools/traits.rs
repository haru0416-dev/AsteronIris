use super::types::{ToolResult, ToolSpec};
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use crate::security::SecurityPolicy;
use crate::security::policy::{AutonomyLevel, EntityRateLimiter, TenantPolicyContext};

/// Core tool trait — implement for any capability
pub trait Tool: Send + Sync {
    /// Tool name (used in LLM function calling)
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON schema for parameters
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with given arguments
    fn execute<'a>(
        &'a self,
        args: Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>>;

    /// Get the full spec for LLM registration
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }
}

/// Runtime context passed to each tool execution.
#[derive(Clone)]
pub struct ExecutionContext {
    pub security: Arc<SecurityPolicy>,
    pub autonomy_level: AutonomyLevel,
    pub entity_id: String,
    pub turn_number: u32,
    pub workspace_dir: PathBuf,
    pub allowed_tools: Option<HashSet<String>>,
    pub rate_limiter: Arc<EntityRateLimiter>,
    pub tenant_context: TenantPolicyContext,
}

impl ExecutionContext {
    pub fn from_security(security: Arc<SecurityPolicy>) -> Self {
        Self {
            workspace_dir: security.workspace_dir.clone(),
            autonomy_level: security.autonomy,
            security,
            entity_id: "default".to_string(),
            turn_number: 0,
            allowed_tools: None,
            rate_limiter: Arc::new(EntityRateLimiter::new(100, 20)),
            tenant_context: TenantPolicyContext::disabled(),
        }
    }
}

#[cfg(test)]
impl ExecutionContext {
    pub fn test_default(security: Arc<SecurityPolicy>) -> Self {
        let mut ctx = Self::from_security(security);
        ctx.entity_id = "test:default".to_string();
        ctx
    }
}

/// Middleware decision for tool execution pipeline.
#[derive(Debug)]
pub enum MiddlewareDecision {
    Continue,
    Block(String),
}

/// Middleware trait for the tool execution pipeline.
pub trait ToolMiddleware: Send + Sync + std::fmt::Debug {
    fn before_execute<'a>(
        &'a self,
        tool_name: &'a str,
        args: &'a Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>>;

    fn after_execute<'a>(
        &'a self,
        tool_name: &'a str,
        result: &'a mut ToolResult,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}
