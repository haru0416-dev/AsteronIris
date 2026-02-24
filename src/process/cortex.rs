use super::deps::AgentDeps;
use super::events::{EventSender, ProcessEvent};
use crate::memory::RecallQuery;
use std::sync::Arc;
use std::time::Duration;

/// Runs the periodic memory distillation loop.
///
/// On each tick, generates a bulletin from recent high-importance memories
/// and stores it in the shared `bulletin_cache`. Exits when the shutdown
/// signal fires.
pub async fn run_cortex_loop(
    deps: AgentDeps,
    events: EventSender,
    interval: Duration,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {
                if let Ok(Some(bulletin)) = generate_bulletin(&deps, "system").await {
                    deps.bulletin_cache.store(Arc::new(Some(bulletin)));
                    let _ = events.send(ProcessEvent::CortexBulletinUpdated);
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break; }
            }
        }
    }
}

/// Generate a bulletin from recent high-importance memories for the entity.
///
/// Returns `Ok(None)` if no relevant memories are found.
pub async fn generate_bulletin(
    deps: &AgentDeps,
    entity_id: &str,
) -> anyhow::Result<Option<String>> {
    let query = RecallQuery::new(entity_id, "recent important context", 10);
    let items = deps.memory.recall_scoped(query).await?;

    if items.is_empty() {
        return Ok(None);
    }

    let bulletin = items
        .iter()
        .map(|item| format!("- {}", item.value))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Some(format!("## Recent Context\n{bulletin}")))
}

#[cfg(test)]
mod tests {
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
    struct EmptyMemory;

    impl Memory for EmptyMemory {
        fn name(&self) -> &str {
            "empty"
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

    #[derive(Debug)]
    struct PopulatedMemory;

    impl Memory for PopulatedMemory {
        fn name(&self) -> &str {
            "populated"
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
            Box::pin(async {
                Ok(vec![
                    MemoryRecallItem {
                        entity_id: "system".to_string(),
                        slot_key: "fact:name".to_string(),
                        value: "User prefers concise responses".to_string(),
                        source: MemorySource::System,
                        confidence: 0.9,
                        importance: 0.8,
                        privacy_level: PrivacyLevel::Public,
                        score: 0.85,
                        occurred_at: "2026-02-24T00:00:00Z".to_string(),
                    },
                    MemoryRecallItem {
                        entity_id: "system".to_string(),
                        slot_key: "fact:project".to_string(),
                        value: "Working on asteroniris project".to_string(),
                        source: MemorySource::System,
                        confidence: 0.95,
                        importance: 0.9,
                        privacy_level: PrivacyLevel::Public,
                        score: 0.90,
                        occurred_at: "2026-02-24T00:00:00Z".to_string(),
                    },
                ])
            })
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
            Box::pin(async { Ok(2) })
        }
    }

    fn make_deps_with_memory(memory: Arc<dyn Memory>) -> AgentDeps {
        let config = Arc::new(Config::default());
        let llm_config = Arc::new(ArcSwap::new(Arc::clone(&config)));
        let llm = Arc::new(LlmManager::new(llm_config));
        let security = Arc::new(SecurityPolicy::default());
        let tool_registry = Arc::new(ToolRegistry::default());

        AgentDeps::new(config, llm, memory, security, tool_registry)
    }

    #[tokio::test]
    async fn generate_bulletin_returns_none_when_empty() {
        let deps = make_deps_with_memory(Arc::new(EmptyMemory));
        let result = generate_bulletin(&deps, "system").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn generate_bulletin_returns_formatted_text() {
        let deps = make_deps_with_memory(Arc::new(PopulatedMemory));
        let result = generate_bulletin(&deps, "system").await.unwrap();
        assert!(result.is_some());

        let bulletin = result.unwrap();
        assert!(bulletin.starts_with("## Recent Context"));
        assert!(bulletin.contains("User prefers concise responses"));
        assert!(bulletin.contains("Working on asteroniris project"));
    }

    #[tokio::test]
    async fn generate_bulletin_produces_dash_prefixed_lines() {
        let deps = make_deps_with_memory(Arc::new(PopulatedMemory));
        let bulletin = generate_bulletin(&deps, "system").await.unwrap().unwrap();

        let lines: Vec<&str> = bulletin.lines().collect();
        // First line is header, rest are dash-prefixed items.
        assert!(lines[1].starts_with("- "));
        assert!(lines[2].starts_with("- "));
    }

    #[tokio::test]
    async fn run_cortex_loop_shuts_down_on_signal() {
        let deps = make_deps_with_memory(Arc::new(EmptyMemory));
        let (tx, _rx) = super::super::events::event_bus(8);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(run_cortex_loop(
            deps,
            tx,
            Duration::from_secs(3600), // long interval so it won't tick
            shutdown_rx,
        ));

        // Signal shutdown.
        shutdown_tx.send(true).unwrap();

        // The loop should exit promptly.
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("cortex loop should exit within 2 seconds")
            .expect("cortex loop should not panic");
    }
}
