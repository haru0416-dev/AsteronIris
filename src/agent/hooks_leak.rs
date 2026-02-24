use super::hooks::{HookDecision, PromptHook};
use crate::llm::leak_detect;
use crate::tools::{ExecutionContext, ToolResult};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// Hook that detects leaked secrets in tool arguments, results, and final output.
///
/// Uses `crate::llm::leak_detect::scan_for_leaks` to scan for known secret
/// patterns (API keys, tokens, credentials) across multiple encodings.
#[derive(Debug)]
pub struct LeakDetectionHook {
    /// Additional literal secrets to scan for (e.g. user-provided API keys).
    secrets: Vec<String>,
}

impl LeakDetectionHook {
    /// Create a new hook with additional literal secrets to watch for.
    pub fn new(secrets: Vec<String>) -> Self {
        Self { secrets }
    }

    /// Scan text for both pattern-based and literal secret matches.
    /// Returns `true` if any secret is found.
    fn contains_secret(&self, text: &str) -> bool {
        // Check pattern-based leaks via the leak_detect module.
        if !leak_detect::scan_for_leaks(text).is_empty() {
            return true;
        }

        // Check literal secrets provided at construction time.
        self.secrets
            .iter()
            .any(|secret| !secret.is_empty() && text.contains(secret.as_str()))
    }
}

impl PromptHook for LeakDetectionHook {
    fn on_tool_call<'a>(
        &'a self,
        tool_name: &'a str,
        args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = HookDecision> + Send + 'a>> {
        Box::pin(async move {
            let serialized = serde_json::to_string(args).unwrap_or_default();
            if self.contains_secret(&serialized) {
                tracing::warn!(
                    tool = tool_name,
                    "Blocked tool call: secret detected in arguments"
                );
                HookDecision::Block(format!(
                    "Tool call to '{tool_name}' blocked: secret detected in arguments"
                ))
            } else {
                HookDecision::Continue
            }
        })
    }

    fn on_tool_result<'a>(
        &'a self,
        tool_name: &'a str,
        result: &'a ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if self.contains_secret(&result.output) {
                tracing::warn!(tool = tool_name, "Secret detected in tool result output");
            }
            if let Some(ref error) = result.error
                && self.contains_secret(error)
            {
                tracing::warn!(tool = tool_name, "Secret detected in tool result error");
            }
        })
    }

    fn on_completion<'a>(
        &'a self,
        final_text: &'a str,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if self.contains_secret(final_text) {
                tracing::warn!("Secret detected in final completion text");
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SecurityPolicy;
    use serde_json::json;
    use std::sync::Arc;

    fn test_ctx() -> ExecutionContext {
        let security = Arc::new(SecurityPolicy::default());
        ExecutionContext::test_default(security)
    }

    #[tokio::test]
    async fn blocks_tool_call_with_pattern_secret() {
        let hook = LeakDetectionHook::new(vec![]);
        let ctx = test_ctx();
        let args = json!({"command": "echo sk-proj_abc123def456ghi789"});

        let decision = hook.on_tool_call("shell", &args, &ctx).await;
        assert!(
            matches!(decision, HookDecision::Block(ref reason) if reason.contains("blocked")),
            "expected Block decision, got {decision:?}"
        );
    }

    #[tokio::test]
    async fn allows_tool_call_without_secrets() {
        let hook = LeakDetectionHook::new(vec![]);
        let ctx = test_ctx();
        let args = json!({"command": "ls -la"});

        let decision = hook.on_tool_call("shell", &args, &ctx).await;
        assert!(matches!(decision, HookDecision::Continue));
    }

    #[tokio::test]
    async fn blocks_tool_call_with_literal_secret() {
        let hook = LeakDetectionHook::new(vec!["my-super-secret-token-12345".to_string()]);
        let ctx = test_ctx();
        let args = json!({"data": "sending my-super-secret-token-12345 to server"});

        let decision = hook.on_tool_call("http", &args, &ctx).await;
        assert!(matches!(decision, HookDecision::Block(_)));
    }

    #[tokio::test]
    async fn on_tool_result_does_not_panic_with_secret() {
        let hook = LeakDetectionHook::new(vec!["secret-value-abc".to_string()]);
        let ctx = test_ctx();
        let result = ToolResult {
            success: true,
            output: "result contains secret-value-abc somewhere".to_string(),
            error: None,
            attachments: Vec::new(),
        };

        // Should log a warning but not panic.
        hook.on_tool_result("shell", &result, &ctx).await;
    }

    #[tokio::test]
    async fn on_completion_does_not_panic_with_secret() {
        let hook = LeakDetectionHook::new(vec![]);
        let ctx = test_ctx();
        let text = "Here is your key: sk-proj_abc123def456ghi789";

        // Should log a warning but not panic.
        hook.on_completion(text, &ctx).await;
    }

    #[tokio::test]
    async fn empty_secret_list_ignores_literals() {
        let hook = LeakDetectionHook::new(vec![]);
        let ctx = test_ctx();
        let args = json!({"data": "nothing secret here"});

        let decision = hook.on_tool_call("test", &args, &ctx).await;
        assert!(matches!(decision, HookDecision::Continue));
    }

    #[tokio::test]
    async fn empty_string_secret_is_ignored() {
        let hook = LeakDetectionHook::new(vec!["".to_string()]);
        let ctx = test_ctx();
        let args = json!({"data": "anything"});

        let decision = hook.on_tool_call("test", &args, &ctx).await;
        assert!(matches!(decision, HookDecision::Continue));
    }

    #[test]
    fn contains_secret_detects_pattern_leaks() {
        let hook = LeakDetectionHook::new(vec![]);
        assert!(hook.contains_secret("token: ghp_ABCDEFghijklmnopqrstuvwxyz1234567890"));
        assert!(!hook.contains_secret("just normal text"));
    }

    #[test]
    fn contains_secret_detects_literal_matches() {
        let hook = LeakDetectionHook::new(vec!["my-literal-secret".to_string()]);
        assert!(hook.contains_secret("this has my-literal-secret in it"));
        assert!(!hook.contains_secret("this is clean"));
    }

    #[test]
    fn leak_detection_hook_debug() {
        let hook = LeakDetectionHook::new(vec!["s1".to_string()]);
        let debug = format!("{hook:?}");
        assert!(debug.contains("LeakDetectionHook"));
    }
}
