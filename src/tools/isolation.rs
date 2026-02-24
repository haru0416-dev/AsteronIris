use super::registry::ToolRegistry;
use super::traits::{ExecutionContext, MiddlewareDecision, Tool, ToolMiddleware};
use super::types::{ToolResult, ToolSpec};
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Per-process tool isolation server.
///
/// Wraps a `ToolRegistry` and enforces namespace isolation so that each
/// worker process only has access to a specific subset of tools. This is
/// the boundary between multi-tenant tool execution and the shared registry.
pub struct ToolServer {
    registry: ToolRegistry,
    namespace: String,
    allowed_tools: HashSet<String>,
}

impl ToolServer {
    /// Create a new `ToolServer` with the given namespace and allowed tool set.
    pub fn new(
        namespace: impl Into<String>,
        allowed_tools: HashSet<String>,
        middleware: Vec<Arc<dyn ToolMiddleware>>,
    ) -> Self {
        Self {
            registry: ToolRegistry::new(middleware),
            namespace: namespace.into(),
            allowed_tools,
        }
    }

    /// Register a tool if it is within the allowed set for this namespace.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> bool {
        if self.allowed_tools.contains(tool.name()) {
            self.registry.register(tool);
            true
        } else {
            false
        }
    }

    /// Get the namespace identifier.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// List tools visible in this namespace.
    pub fn tool_names(&self) -> Vec<&str> {
        self.registry.tool_names()
    }

    /// Get specs for all tools in this namespace.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.registry.specs()
    }

    /// Execute a tool, enforcing namespace isolation.
    ///
    /// Returns an error result if the tool is not in the allowed set.
    pub async fn execute(
        &self,
        name: &str,
        args: Value,
        ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        if !self.allowed_tools.contains(name) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "Tool '{name}' is not available in namespace '{}'",
                    self.namespace
                )),
                attachments: Vec::new(),
            });
        }

        self.registry.execute(name, args, ctx).await
    }
}

/// Middleware that restricts execution to a tool namespace.
#[derive(Debug)]
pub struct NamespaceMiddleware {
    allowed: HashSet<String>,
    namespace: String,
}

impl NamespaceMiddleware {
    pub fn new(namespace: impl Into<String>, allowed: HashSet<String>) -> Self {
        Self {
            allowed,
            namespace: namespace.into(),
        }
    }
}

impl ToolMiddleware for NamespaceMiddleware {
    fn before_execute<'a>(
        &'a self,
        tool_name: &'a str,
        _args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move {
            if self.allowed.contains(tool_name) {
                Ok(MiddlewareDecision::Continue)
            } else {
                Ok(MiddlewareDecision::Block(format!(
                    "Tool '{tool_name}' is not available in namespace '{}'",
                    self.namespace
                )))
            }
        })
    }

    fn after_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityPolicy;
    use crate::tools::traits::ExecutionContext;
    use serde_json::json;

    #[derive(Debug)]
    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo test tool"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn execute<'a>(
            &'a self,
            _args: Value,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
            Box::pin(async move {
                Ok(ToolResult {
                    success: true,
                    output: "echo".to_string(),
                    error: None,
                    attachments: Vec::new(),
                })
            })
        }
    }

    #[tokio::test]
    async fn tool_server_rejects_disallowed_tool() {
        let mut server = ToolServer::new("test-ns", HashSet::from(["echo".to_string()]), vec![]);
        server.register(Box::new(EchoTool));

        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);

        let result = server
            .execute("not_allowed", json!({}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(
            result
                .error
                .as_deref()
                .is_some_and(|msg| msg.contains("not available"))
        );
    }

    #[tokio::test]
    async fn tool_server_allows_registered_tool() {
        let mut server = ToolServer::new("test-ns", HashSet::from(["echo".to_string()]), vec![]);
        server.register(Box::new(EchoTool));

        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);

        let result = server.execute("echo", json!({}), &ctx).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "echo");
    }

    #[tokio::test]
    async fn tool_server_register_respects_allowed_set() {
        let mut server =
            ToolServer::new("restricted", HashSet::from(["other".to_string()]), vec![]);
        let registered = server.register(Box::new(EchoTool));
        assert!(!registered);
        assert!(server.tool_names().is_empty());
    }

    #[test]
    fn namespace_returns_correct_value() {
        let server = ToolServer::new("my-ns", HashSet::new(), vec![]);
        assert_eq!(server.namespace(), "my-ns");
    }

    #[tokio::test]
    async fn namespace_middleware_blocks_disallowed() {
        let mw = NamespaceMiddleware::new("ns", HashSet::from(["allowed".to_string()]));
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);

        let decision = mw
            .before_execute("blocked_tool", &json!({}), &ctx)
            .await
            .unwrap();
        assert!(matches!(decision, MiddlewareDecision::Block(_)));
    }

    #[tokio::test]
    async fn namespace_middleware_allows_permitted() {
        let mw = NamespaceMiddleware::new("ns", HashSet::from(["allowed".to_string()]));
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);

        let decision = mw
            .before_execute("allowed", &json!({}), &ctx)
            .await
            .unwrap();
        assert!(matches!(decision, MiddlewareDecision::Continue));
    }
}
