use crate::security::{ApprovalDecision, ApprovalRequest, classify_risk};
use crate::tools::middleware::{ExecutionContext, MiddlewareDecision, ToolMiddleware};
use crate::tools::traits::{ActionIntent, Tool, ToolResult, ToolSpec};
use anyhow::Context;
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
                MiddlewareDecision::RequireApproval(intent) => {
                    let Some(broker) = &ctx.approval_broker else {
                        anyhow::bail!(
                            "tool execution requires approval: action_kind='{}' intent_id='{}'",
                            intent.action_kind,
                            intent.intent_id
                        );
                    };

                    let request = approval_request_from_intent(&intent);
                    match broker.request_approval(&request).await? {
                        ApprovalDecision::Approved => {}
                        ApprovalDecision::ApprovedWithGrant(grant) => {
                            if let Some(permission_store) = &ctx.permission_store {
                                permission_store
                                    .add_grant(grant, &ctx.entity_id)
                                    .with_context(|| {
                                        format!(
                                            "failed to persist approval grant for entity '{}'",
                                            ctx.entity_id
                                        )
                                    })?;
                            }
                        }
                        ApprovalDecision::Denied { reason } => {
                            return Ok(ToolResult {
                                success: false,
                                output: String::new(),
                                error: Some(format!(
                                    "tool execution denied by approval broker: {reason}"
                                )),

                                attachments: Vec::new(),
                            });
                        }
                    }
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

fn approval_request_from_intent(intent: &ActionIntent) -> ApprovalRequest {
    let tool_name = intent.action_kind.clone();
    let args_summary = intent
        .payload
        .get("args_summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let entity_id = intent.operator.clone();
    let channel = entity_id.split(':').next().unwrap_or("unknown").to_string();

    ApprovalRequest {
        intent_id: intent.intent_id.clone(),
        tool_name: tool_name.clone(),
        args_summary,
        risk_level: classify_risk(&tool_name),
        entity_id,
        channel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{
        ApprovalBroker, ApprovalDecision, ApprovalRequest, GrantScope, PermissionGrant,
        PermissionStore, RiskLevel, SecurityPolicy,
    };
    use crate::tools::middleware::{ExecutionContext, SecurityMiddleware};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

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

                attachments: Vec::new(),
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

    #[derive(Debug)]
    struct RequireApprovalMiddleware;

    #[async_trait]
    impl ToolMiddleware for RequireApprovalMiddleware {
        async fn before_execute(
            &self,
            _tool_name: &str,
            _args: &Value,
            _ctx: &ExecutionContext,
        ) -> anyhow::Result<MiddlewareDecision> {
            Ok(MiddlewareDecision::RequireApproval(ActionIntent::new(
                "shell",
                "discord:user-1",
                json!({"args_summary": "cargo test"}),
            )))
        }

        async fn after_execute(
            &self,
            _tool_name: &str,
            _result: &mut ToolResult,
            _ctx: &ExecutionContext,
        ) {
        }
    }

    #[derive(Debug)]
    struct CountingBroker {
        calls: Arc<AtomicUsize>,
        decision: ApprovalDecision,
        last_request: Arc<Mutex<Option<ApprovalRequest>>>,
    }

    #[async_trait]
    impl ApprovalBroker for CountingBroker {
        async fn request_approval(
            &self,
            request: &ApprovalRequest,
        ) -> anyhow::Result<ApprovalDecision> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut slot = self
                .last_request
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *slot = Some(request.clone());
            Ok(self.decision.clone())
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

    #[tokio::test]
    async fn execute_require_approval_without_broker_keeps_backward_compat_bail() {
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let mut registry = ToolRegistry::new(vec![Arc::new(RequireApprovalMiddleware)]);
        registry.register(Box::new(TestTool));

        let error = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("requires approval"));
    }

    #[tokio::test]
    async fn execute_require_approval_with_broker_approved_executes_tool() {
        let security = Arc::new(SecurityPolicy::default());
        let mut ctx = ExecutionContext::test_default(security);
        ctx.approval_broker = Some(Arc::new(CountingBroker {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: ApprovalDecision::Approved,
            last_request: Arc::new(Mutex::new(None)),
        }));

        let mut registry = ToolRegistry::new(vec![Arc::new(RequireApprovalMiddleware)]);
        registry.register(Box::new(TestTool));

        let result = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "ok");
    }

    #[tokio::test]
    async fn execute_require_approval_with_broker_denied_returns_tool_result_error() {
        let security = Arc::new(SecurityPolicy::default());
        let mut ctx = ExecutionContext::test_default(security);
        ctx.approval_broker = Some(Arc::new(CountingBroker {
            calls: Arc::new(AtomicUsize::new(0)),
            decision: ApprovalDecision::Denied {
                reason: "operator rejected".to_string(),
            },
            last_request: Arc::new(Mutex::new(None)),
        }));

        let mut registry = ToolRegistry::new(vec![Arc::new(RequireApprovalMiddleware)]);
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
                .is_some_and(|msg| msg.contains("operator rejected"))
        );
    }

    #[tokio::test]
    async fn approval_request_mapping_uses_intent_action_kind_entity_and_risk() {
        let security = Arc::new(SecurityPolicy::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let last_request = Arc::new(Mutex::new(None));
        let mut ctx = ExecutionContext::test_default(security);
        ctx.approval_broker = Some(Arc::new(CountingBroker {
            calls: Arc::clone(&calls),
            decision: ApprovalDecision::Approved,
            last_request: Arc::clone(&last_request),
        }));

        let mut registry = ToolRegistry::new(vec![Arc::new(RequireApprovalMiddleware)]);
        registry.register(Box::new(TestTool));

        let _ = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let request = last_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap();
        assert_eq!(request.tool_name, "shell");
        assert_eq!(request.entity_id, "discord:user-1");
        assert_eq!(request.channel, "discord");
        assert_eq!(request.risk_level, RiskLevel::High);
        assert_eq!(request.args_summary, "cargo test");
    }

    #[tokio::test]
    async fn execute_with_grant_stores_permission_and_future_call_skips_broker() {
        let security = Arc::new(SecurityPolicy::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let temp_dir = TempDir::new().unwrap();
        let permission_store = Arc::new(PermissionStore::load(temp_dir.path()));
        let mut ctx = ExecutionContext::test_default(security);
        ctx.permission_store = Some(Arc::clone(&permission_store));
        ctx.approval_broker = Some(Arc::new(CountingBroker {
            calls: Arc::clone(&calls),
            decision: ApprovalDecision::ApprovedWithGrant(PermissionGrant {
                tool: "test_tool".to_string(),
                pattern: "*".to_string(),
                scope: GrantScope::Session,
            }),
            last_request: Arc::new(Mutex::new(None)),
        }));

        let mut registry = ToolRegistry::new(vec![Arc::new(SecurityMiddleware)]);
        registry.register(Box::new(TestTool));

        let first = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();
        assert!(first.success);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(permission_store.is_granted("test_tool", "{}"));

        let second = registry
            .execute("test_tool", json!({}), &ctx)
            .await
            .unwrap();
        assert!(second.success);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
