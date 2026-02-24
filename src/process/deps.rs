use crate::agent::PromptHook;
use crate::config::Config;
use crate::llm::manager::LlmManager;
use crate::memory::traits::Memory;
use crate::security::SecurityPolicy;
use crate::tools::ToolRegistry;
use arc_swap::ArcSwap;
use std::sync::Arc;

/// Shared dependencies for all process components.
///
/// Cheaply cloneable via `Arc` internals. Thread-safe by construction.
#[derive(Clone)]
pub struct AgentDeps {
    pub config: Arc<Config>,
    pub llm: Arc<LlmManager>,
    pub memory: Arc<dyn Memory>,
    pub security: Arc<SecurityPolicy>,
    pub tool_registry: Arc<ToolRegistry>,
    pub hooks: Vec<Arc<dyn PromptHook>>,
    pub bulletin_cache: Arc<ArcSwap<Option<String>>>,
}

impl AgentDeps {
    pub fn new(
        config: Arc<Config>,
        llm: Arc<LlmManager>,
        memory: Arc<dyn Memory>,
        security: Arc<SecurityPolicy>,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            config,
            llm,
            memory,
            security,
            tool_registry,
            hooks: Vec::new(),
            bulletin_cache: Arc::new(ArcSwap::new(Arc::new(None))),
        }
    }

    pub fn with_hooks(mut self, hooks: Vec<Arc<dyn PromptHook>>) -> Self {
        self.hooks = hooks;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::traits::*;
    use std::future::Future;
    use std::pin::Pin;

    #[derive(Debug)]
    struct StubMemory;

    impl Memory for StubMemory {
        fn name(&self) -> &str {
            "stub"
        }

        fn health_check(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
            Box::pin(async { true })
        }

        fn append_event(
            &self,
            _input: MemoryEventInput,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<MemoryEvent>> + Send + '_>> {
            Box::pin(async { anyhow::bail!("stub") })
        }

        fn recall_scoped(
            &self,
            _query: RecallQuery,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryRecallItem>>> + Send + '_>>
        {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn resolve_slot<'a>(
            &'a self,
            _entity_id: &'a str,
            _slot_key: &'a str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<BeliefSlot>>> + Send + 'a>> {
            Box::pin(async { Ok(None) })
        }

        fn forget_slot<'a>(
            &'a self,
            _entity_id: &'a str,
            _slot_key: &'a str,
            _mode: ForgetMode,
            _reason: &'a str,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<ForgetOutcome>> + Send + 'a>> {
            Box::pin(async move {
                Ok(ForgetOutcome {
                    entity_id: _entity_id.to_string(),
                    slot_key: _slot_key.to_string(),
                    mode: _mode,
                    applied: false,
                    complete: false,
                    degraded: false,
                    status: ForgetStatus::NotApplied,
                    artifact_checks: Vec::new(),
                })
            })
        }

        fn count_events<'a>(
            &'a self,
            _entity_id: Option<&'a str>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<usize>> + Send + 'a>> {
            Box::pin(async { Ok(0) })
        }
    }

    fn make_deps() -> AgentDeps {
        use arc_swap::ArcSwap;

        let config = Arc::new(Config::default());
        let llm_config = Arc::new(ArcSwap::new(Arc::clone(&config)));
        let llm = Arc::new(LlmManager::new(llm_config));
        let memory: Arc<dyn Memory> = Arc::new(StubMemory);
        let security = Arc::new(SecurityPolicy::default());
        let tool_registry = Arc::new(ToolRegistry::default());

        AgentDeps::new(config, llm, memory, security, tool_registry)
    }

    #[test]
    fn new_creates_deps_with_empty_hooks() {
        let deps = make_deps();
        assert!(deps.hooks.is_empty());
    }

    #[test]
    fn with_hooks_sets_hooks_vec() {
        use crate::agent::hooks::{HookDecision, PromptHook};
        use crate::tools::{ExecutionContext, ToolResult};
        use serde_json::Value;

        #[derive(Debug)]
        struct NoopHook;

        impl PromptHook for NoopHook {
            fn on_tool_call<'a>(
                &'a self,
                _tool_name: &'a str,
                _args: &'a Value,
                _ctx: &'a ExecutionContext,
            ) -> Pin<Box<dyn Future<Output = HookDecision> + Send + 'a>> {
                Box::pin(async { HookDecision::Continue })
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

        let deps = make_deps().with_hooks(vec![Arc::new(NoopHook)]);
        assert_eq!(deps.hooks.len(), 1);
    }

    #[test]
    fn bulletin_cache_defaults_to_none() {
        let deps = make_deps();
        let cached = deps.bulletin_cache.load();
        assert!(cached.is_none());
    }

    #[test]
    fn clone_shares_arc_internals() {
        let deps = make_deps();
        let cloned = deps.clone();
        assert!(Arc::ptr_eq(&deps.config, &cloned.config));
        assert!(Arc::ptr_eq(&deps.llm, &cloned.llm));
        assert!(Arc::ptr_eq(&deps.security, &cloned.security));
        assert!(Arc::ptr_eq(&deps.tool_registry, &cloned.tool_registry));
        assert!(Arc::ptr_eq(&deps.bulletin_cache, &cloned.bulletin_cache));
    }
}
