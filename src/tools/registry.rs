use crate::tools::middleware::{ExecutionContext, MiddlewareDecision, ToolMiddleware};
use crate::tools::traits::{Tool, ToolResult, ToolSpec};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

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

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let tool: Arc<dyn Tool> = Arc::from(tool);
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn unregister(&mut self, name: &str) -> bool {
        self.tools.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn tool_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.tools.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

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
                    });
                }
                MiddlewareDecision::RequireApproval(intent) => {
                    anyhow::bail!(
                        "tool execution requires approval: action_kind='{}' intent_id='{}'",
                        intent.action_kind,
                        intent.intent_id
                    );
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
    use crate::tools::middleware::{ExecutionContext, SecurityMiddleware};
    use async_trait::async_trait;
    use serde_json::json;

    #[derive(Debug)]
    struct TestTool;

    #[async_trait]
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

        async fn execute(
            &self,
            _args: Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                success: true,
                output: "ok".to_string(),
                error: None,
            })
        }
    }

    #[derive(Debug)]
    struct BlockAllMiddleware;

    #[async_trait]
    impl ToolMiddleware for BlockAllMiddleware {
        async fn before_execute(
            &self,
            _tool_name: &str,
            _args: &Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<MiddlewareDecision> {
            Ok(MiddlewareDecision::Block("blocked".to_string()))
        }

        async fn after_execute(
            &self,
            _tool_name: &str,
            _result: &mut ToolResult,
            _ctx: &ExecutionContext,
        ) {
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
    async fn execute_with_security_middleware_blocks_readonly() {
        let security = Arc::new(SecurityPolicy::default());
        let mut ctx = ExecutionContext::test_default(security);
        ctx.autonomy_level = crate::security::AutonomyLevel::ReadOnly;

        let mut registry = ToolRegistry::new(vec![Arc::new(SecurityMiddleware)]);
        registry.register(Box::new(TestTool));

        let result = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|msg| msg.contains("read-only"))
        );
    }
}
