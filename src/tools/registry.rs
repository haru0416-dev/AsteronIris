use super::traits::{ExecutionContext, MiddlewareDecision, Tool, ToolMiddleware};
use super::types::{ToolResult, ToolSpec};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Central registry for tool instances and middleware pipeline.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    middleware: Vec<Arc<dyn ToolMiddleware>>,
}

impl ToolRegistry {
    pub fn new(middleware: Vec<Arc<dyn ToolMiddleware>>) -> Self {
        Self {
            tools: HashMap::new(),
            middleware,
        }
    }

    /// Register a tool. Replaces any existing tool with the same name.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let tool: Arc<dyn Tool> = Arc::from(tool);
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Remove a tool by name. Returns whether it was present.
    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Return sorted list of registered tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// Return specs for all registered tools.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    /// Return specs filtered by the execution context's allowed-tools set.
    pub fn specs_for_context(&self, ctx: &ExecutionContext) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .filter(|(name, _)| {
                ctx.allowed_tools
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(*name))
            })
            .map(|(_, tool)| tool.spec())
            .collect()
    }

    /// Execute a tool through the middleware pipeline.
    pub async fn execute(
        &self,
        name: &str,
        args: Value,
        ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let Some(tool) = self.tools.get(name) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Tool not found: {name}")),
                attachments: Vec::new(),
            });
        };

        for middleware in &self.middleware {
            match middleware.before_execute(name, &args, ctx).await? {
                MiddlewareDecision::Continue => {}
                MiddlewareDecision::Block(reason) => {
                    return Ok(ToolResult {
                        success: false,
                        output: String::new(),
                        error: Some(reason),
                        attachments: Vec::new(),
                    });
                }
            }
        }

        let mut result = tool.execute(args, ctx).await?;

        for middleware in &self.middleware {
            middleware.after_execute(name, &mut result, ctx).await;
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityPolicy;
    use crate::tools::traits::{ExecutionContext, MiddlewareDecision, Tool, ToolMiddleware};
    use crate::tools::types::ToolResult;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestTool;

    impl Tool for TestTool {
        fn name(&self) -> &str {
            "test_tool"
        }

        fn description(&self) -> &str {
            "test"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn execute<'a>(
            &'a self,
            _args: Value,
            _ctx: &'a ExecutionContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<ToolResult>> + Send + 'a>,
        > {
            Box::pin(async move {
                Ok(ToolResult {
                    success: true,
                    output: "ok".to_string(),
                    error: None,
                    attachments: Vec::new(),
                })
            })
        }
    }

    #[derive(Debug)]
    struct BlockAllMiddleware;

    impl ToolMiddleware for BlockAllMiddleware {
        fn before_execute<'a>(
            &'a self,
            _tool_name: &'a str,
            _args: &'a Value,
            _ctx: &'a ExecutionContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>,
        > {
            Box::pin(async move { Ok(MiddlewareDecision::Block("blocked".to_string())) })
        }

        fn after_execute<'a>(
            &'a self,
            _tool_name: &'a str,
            _result: &'a mut ToolResult,
            _ctx: &'a ExecutionContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {})
        }
    }

    #[tokio::test]
    async fn execute_runs_middleware_chain() {
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let mut registry = ToolRegistry::new(vec![Arc::new(BlockAllMiddleware)]);
        registry.register(Box::new(TestTool));

        let result = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("blocked"));
    }

    #[tokio::test]
    async fn specs_for_context_filters_allowed_tools() {
        let security = Arc::new(SecurityPolicy::default());
        let mut ctx = ExecutionContext::test_default(security);
        ctx.allowed_tools = Some(std::collections::HashSet::from(["test_tool".to_string()]));

        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(TestTool));

        let specs = registry.specs_for_context(&ctx);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "test_tool");
    }

    #[tokio::test]
    async fn execute_returns_error_for_unknown_tool() {
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let registry = ToolRegistry::new(vec![]);

        let result = registry
            .execute("nonexistent", json!({}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|msg| msg.contains("Tool not found"))
        );
    }

    #[tokio::test]
    async fn execute_without_middleware_runs_tool_directly() {
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let mut registry = ToolRegistry::new(vec![]);
        registry.register(Box::new(TestTool));

        let result = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "ok");
    }
}
