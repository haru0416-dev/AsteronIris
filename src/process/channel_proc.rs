use super::branch::Branch;
use super::deps::AgentDeps;
use super::events::{EventSender, ProcessEvent};
use super::worker::WorkerResult;
use std::collections::HashMap;

/// Channel-level process that routes incoming messages to per-entity [`Branch`]es.
///
/// Each entity gets its own conversation branch. Branches are created on demand
/// when the first message arrives for a given `entity_id`.
pub struct ChannelProcess {
    branches: HashMap<String, Branch>,
    deps: AgentDeps,
    events: EventSender,
    default_model: String,
    default_temperature: f64,
    system_prompt: String,
}

impl ChannelProcess {
    pub fn new(
        deps: AgentDeps,
        events: EventSender,
        system_prompt: impl Into<String>,
        default_model: impl Into<String>,
        default_temperature: f64,
    ) -> Self {
        Self {
            branches: HashMap::new(),
            deps,
            events,
            default_model: default_model.into(),
            default_temperature,
            system_prompt: system_prompt.into(),
        }
    }

    /// Handle an incoming message from the given entity.
    ///
    /// Creates a new branch if this is the first message from this entity.
    pub async fn handle_message(
        &mut self,
        entity_id: &str,
        message: &str,
    ) -> anyhow::Result<WorkerResult> {
        if !self.branches.contains_key(entity_id) {
            let branch = Branch::new(
                entity_id.to_string(),
                self.deps.clone(),
                self.events.clone(),
            );
            let _ = self.events.send(ProcessEvent::BranchCreated {
                entity_id: entity_id.to_string(),
            });
            self.branches.insert(entity_id.to_string(), branch);
        }

        let branch = self
            .branches
            .get_mut(entity_id)
            .expect("branch was just inserted");
        branch
            .process_message(
                message,
                &self.system_prompt,
                &self.default_model,
                self.default_temperature,
            )
            .await
    }

    /// Close and remove the branch for the given entity.
    ///
    /// Returns `true` if the branch existed and was removed.
    pub fn close_branch(&mut self, entity_id: &str) -> bool {
        if self.branches.remove(entity_id).is_some() {
            let _ = self.events.send(ProcessEvent::BranchClosed {
                entity_id: entity_id.to_string(),
            });
            true
        } else {
            false
        }
    }

    /// Return the entity IDs of all active branches.
    pub fn active_entities(&self) -> Vec<&str> {
        self.branches.keys().map(String::as_str).collect()
    }

    /// Return the number of active branches.
    pub fn branch_count(&self) -> usize {
        self.branches.len()
    }
}

#[cfg(test)]
mod tests {
    use super::super::events::event_bus;
    use super::*;
    use crate::config::Config;
    use crate::llm::manager::LlmManager;
    use crate::memory::traits::*;
    use crate::security::SecurityPolicy;
    use crate::tools::ToolRegistry;
    use arc_swap::ArcSwap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

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
        let config = Arc::new(Config::default());
        let llm_config = Arc::new(ArcSwap::new(Arc::clone(&config)));
        let llm = Arc::new(LlmManager::new(llm_config));
        let memory: Arc<dyn Memory> = Arc::new(StubMemory);
        let security = Arc::new(SecurityPolicy::default());
        let tool_registry = Arc::new(ToolRegistry::default());

        AgentDeps::new(config, llm, memory, security, tool_registry)
    }

    #[test]
    fn new_channel_process_starts_empty() {
        let deps = make_deps();
        let (tx, _rx) = event_bus(8);
        let proc = ChannelProcess::new(deps, tx, "You are helpful.", "test-model", 0.7);

        assert_eq!(proc.branch_count(), 0);
        assert!(proc.active_entities().is_empty());
    }

    #[test]
    fn close_branch_returns_false_for_nonexistent() {
        let deps = make_deps();
        let (tx, _rx) = event_bus(8);
        let mut proc = ChannelProcess::new(deps, tx, "prompt", "model", 0.7);

        assert!(!proc.close_branch("nobody"));
    }

    #[tokio::test]
    async fn close_branch_emits_event() {
        let deps = make_deps();
        let (tx, mut rx) = event_bus(16);
        let mut proc = ChannelProcess::new(deps, tx, "prompt", "model", 0.7);

        // Manually insert a branch so we don't need a working provider.
        let branch_deps = proc.deps.clone();
        let branch_events = proc.events.clone();
        proc.branches.insert(
            "user:1".to_string(),
            Branch::new("user:1".to_string(), branch_deps, branch_events),
        );

        assert_eq!(proc.branch_count(), 1);
        assert!(proc.close_branch("user:1"));
        assert_eq!(proc.branch_count(), 0);

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, ProcessEvent::BranchClosed { .. }));
    }

    #[test]
    fn active_entities_returns_keys() {
        let deps = make_deps();
        let (tx, _rx) = event_bus(8);
        let mut proc = ChannelProcess::new(deps, tx, "prompt", "model", 0.7);

        let branch_deps = proc.deps.clone();
        let branch_events = proc.events.clone();
        proc.branches.insert(
            "user:a".to_string(),
            Branch::new(
                "user:a".to_string(),
                branch_deps.clone(),
                branch_events.clone(),
            ),
        );
        proc.branches.insert(
            "user:b".to_string(),
            Branch::new("user:b".to_string(), branch_deps, branch_events),
        );

        let mut entities = proc.active_entities();
        entities.sort_unstable();
        assert_eq!(entities, vec!["user:a", "user:b"]);
    }
}
