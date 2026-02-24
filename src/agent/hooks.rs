use crate::tools::ExecutionContext;
use crate::tools::ToolResult;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// Decision returned by a hook before tool execution.
#[derive(Debug, Clone)]
pub enum HookDecision {
    /// Allow the tool call to proceed.
    Continue,
    /// Block the tool call with the given reason.
    Block(String),
}

/// Lifecycle hook for the agent tool loop.
///
/// Hooks are invoked at three points:
/// 1. Before each tool execution (`on_tool_call`)
/// 2. After each tool result (`on_tool_result`)
/// 3. When the loop completes (`on_completion`)
pub trait PromptHook: Send + Sync + std::fmt::Debug {
    /// Called before a tool is executed. Return `Block(reason)` to skip execution.
    fn on_tool_call<'a>(
        &'a self,
        tool_name: &'a str,
        args: &'a Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = HookDecision> + Send + 'a>>;

    /// Called after a tool produces a result.
    fn on_tool_result<'a>(
        &'a self,
        tool_name: &'a str,
        result: &'a ToolResult,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    /// Called when the tool loop finishes with a final text response.
    fn on_completion<'a>(
        &'a self,
        final_text: &'a str,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityPolicy;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug)]
    struct CountingHook {
        call_count: AtomicU32,
        result_count: AtomicU32,
        completion_count: AtomicU32,
    }

    impl CountingHook {
        fn new() -> Self {
            Self {
                call_count: AtomicU32::new(0),
                result_count: AtomicU32::new(0),
                completion_count: AtomicU32::new(0),
            }
        }
    }

    impl PromptHook for CountingHook {
        fn on_tool_call<'a>(
            &'a self,
            _tool_name: &'a str,
            _args: &'a Value,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = HookDecision> + Send + 'a>> {
            Box::pin(async move {
                self.call_count.fetch_add(1, Ordering::Relaxed);
                HookDecision::Continue
            })
        }

        fn on_tool_result<'a>(
            &'a self,
            _tool_name: &'a str,
            _result: &'a ToolResult,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {
                self.result_count.fetch_add(1, Ordering::Relaxed);
            })
        }

        fn on_completion<'a>(
            &'a self,
            _final_text: &'a str,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async move {
                self.completion_count.fetch_add(1, Ordering::Relaxed);
            })
        }
    }

    #[tokio::test]
    async fn counting_hook_on_tool_call_increments() {
        let hook = CountingHook::new();
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let args = serde_json::json!({});

        let decision = hook.on_tool_call("shell", &args, &ctx).await;
        assert!(matches!(decision, HookDecision::Continue));
        assert_eq!(hook.call_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn counting_hook_on_tool_result_increments() {
        let hook = CountingHook::new();
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let result = ToolResult {
            success: true,
            output: "ok".to_string(),
            error: None,
            attachments: Vec::new(),
        };

        hook.on_tool_result("shell", &result, &ctx).await;
        assert_eq!(hook.result_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn counting_hook_on_completion_increments() {
        let hook = CountingHook::new();
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);

        hook.on_completion("done", &ctx).await;
        assert_eq!(hook.completion_count.load(Ordering::Relaxed), 1);
    }

    #[derive(Debug)]
    struct BlockingHook;

    impl PromptHook for BlockingHook {
        fn on_tool_call<'a>(
            &'a self,
            _tool_name: &'a str,
            _args: &'a Value,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = HookDecision> + Send + 'a>> {
            Box::pin(async move { HookDecision::Block("denied by policy".to_string()) })
        }

        fn on_tool_result<'a>(
            &'a self,
            _tool_name: &'a str,
            _result: &'a ToolResult,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async {})
        }

        fn on_completion<'a>(
            &'a self,
            _final_text: &'a str,
            _ctx: &'a ExecutionContext,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            Box::pin(async {})
        }
    }

    #[tokio::test]
    async fn blocking_hook_returns_block_decision() {
        let hook = BlockingHook;
        let security = Arc::new(SecurityPolicy::default());
        let ctx = ExecutionContext::test_default(security);
        let args = serde_json::json!({});

        let decision = hook.on_tool_call("shell", &args, &ctx).await;
        assert!(matches!(decision, HookDecision::Block(reason) if reason == "denied by policy"));
    }

    #[test]
    fn hook_decision_debug_and_clone() {
        let cont = HookDecision::Continue;
        let cloned = cont.clone();
        assert!(matches!(cloned, HookDecision::Continue));
        let _ = format!("{cont:?}");

        let block = HookDecision::Block("reason".to_string());
        let cloned = block.clone();
        assert!(matches!(cloned, HookDecision::Block(ref r) if r == "reason"));
        let _ = format!("{block:?}");
    }
}
