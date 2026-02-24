use super::deps::AgentDeps;
use super::events::EventSender;
use super::worker::{WorkerParams, WorkerResult, run_worker};
use crate::llm::types::{ContentBlock, MessageRole, ProviderMessage};

/// Per-entity conversation state.
///
/// Maintains the running conversation history for a single entity and
/// delegates each turn to [`run_worker`].
pub struct Branch {
    entity_id: String,
    conversation_history: Vec<ProviderMessage>,
    turn_count: u32,
    deps: AgentDeps,
    events: EventSender,
}

impl Branch {
    pub fn new(entity_id: String, deps: AgentDeps, events: EventSender) -> Self {
        Self {
            entity_id,
            conversation_history: Vec::new(),
            turn_count: 0,
            deps,
            events,
        }
    }

    /// Process a single user message: run the worker, append to history.
    pub async fn process_message(
        &mut self,
        message: &str,
        system_prompt: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<WorkerResult> {
        self.turn_count += 1;

        let params = WorkerParams {
            entity_id: self.entity_id.clone(),
            system_prompt: system_prompt.to_string(),
            user_message: message.to_string(),
            image_content: Vec::new(),
            model: model.to_string(),
            temperature,
            max_tool_iterations: 25,
            conversation_history: self.conversation_history.clone(),
            stream_sink: None,
        };

        let result = run_worker(&self.deps, params, &self.events).await?;

        // Append user message to history.
        self.conversation_history.push(ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: message.to_string(),
            }],
        });

        // Append assistant response to history.
        if !result.tool_loop_result.final_text.is_empty() {
            self.conversation_history.push(ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: result.tool_loop_result.final_text.clone(),
                }],
            });
        }

        Ok(result)
    }

    pub fn history_len(&self) -> usize {
        self.conversation_history.len()
    }

    pub fn entity_id(&self) -> &str {
        &self.entity_id
    }

    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    pub fn set_history(&mut self, history: Vec<ProviderMessage>) {
        self.conversation_history = history;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Branch is tested at the integration level since it requires a working
    // LlmManager with a real or mock provider. Unit tests focus on the
    // state-management methods.

    use super::super::events::event_bus;
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
        use super::super::deps::AgentDeps;

        let config = Arc::new(Config::default());
        let llm_config = Arc::new(ArcSwap::new(Arc::clone(&config)));
        let llm = Arc::new(LlmManager::new(llm_config));
        let memory: Arc<dyn Memory> = Arc::new(StubMemory);
        let security = Arc::new(SecurityPolicy::default());
        let tool_registry = Arc::new(ToolRegistry::default());

        AgentDeps::new(config, llm, memory, security, tool_registry)
    }

    #[test]
    fn new_branch_starts_empty() {
        let deps = make_deps();
        let (tx, _rx) = event_bus(8);
        let branch = Branch::new("user:1".to_string(), deps, tx);

        assert_eq!(branch.entity_id(), "user:1");
        assert_eq!(branch.turn_count(), 0);
        assert_eq!(branch.history_len(), 0);
    }

    #[test]
    fn set_history_replaces_conversation() {
        let deps = make_deps();
        let (tx, _rx) = event_bus(8);
        let mut branch = Branch::new("user:1".to_string(), deps, tx);

        let history = vec![
            ProviderMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "hello".to_string(),
                }],
            },
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "hi".to_string(),
                }],
            },
        ];

        branch.set_history(history);
        assert_eq!(branch.history_len(), 2);
    }
}
