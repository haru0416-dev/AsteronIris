pub mod coordination;
pub mod dispatch;
pub mod roles;

pub use coordination::{
    AggregatedResult, CoordinationManager, CoordinationSession, DispatchResult, SharedContext,
};
pub use dispatch::dispatch_parallel;
pub use roles::{AgentRole, RoleAssignment, RoleConfig};

use crate::llm::traits::Provider;
use anyhow::{Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentRunSnapshot {
    pub run_id: String,
    pub label: Option<String>,
    pub task: String,
    pub model: String,
    pub status: SubagentRunStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

struct SubagentRunEntry {
    snapshot: SubagentRunSnapshot,
    handle: Option<JoinHandle<()>>,
}

/// Instance-based subagent runtime. No global statics.
pub struct SubagentRuntime {
    provider: Arc<dyn Provider>,
    system_prompt: String,
    default_model: String,
    default_temperature: f64,
    runs: Mutex<HashMap<String, SubagentRunEntry>>,
}

impl SubagentRuntime {
    pub fn new(
        provider: Arc<dyn Provider>,
        system_prompt: impl Into<String>,
        default_model: impl Into<String>,
        default_temperature: f64,
    ) -> Self {
        Self {
            provider,
            system_prompt: system_prompt.into(),
            default_model: default_model.into(),
            default_temperature,
            runs: Mutex::new(HashMap::new()),
        }
    }

    pub async fn run_inline(&self, task: String, model: Option<String>) -> Result<String> {
        let model_name = model.unwrap_or_else(|| self.default_model.clone());
        self.provider
            .chat_with_system(
                Some(&self.system_prompt),
                &task,
                &model_name,
                self.default_temperature,
            )
            .await
    }

    pub fn spawn(
        self: &Arc<Self>,
        task: String,
        label: Option<String>,
        model: Option<String>,
    ) -> Result<SubagentRunSnapshot> {
        let run_id = format!("subagent_{}", Uuid::new_v4().simple());
        let model_name = model.unwrap_or_else(|| self.default_model.clone());
        let snapshot = SubagentRunSnapshot {
            run_id: run_id.clone(),
            label,
            task: task.clone(),
            model: model_name.clone(),
            status: SubagentRunStatus::Running,
            output: None,
            error: None,
            started_at: Utc::now().to_rfc3339(),
            finished_at: None,
        };

        {
            let mut runs = self
                .runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            runs.insert(
                run_id.clone(),
                SubagentRunEntry {
                    snapshot: snapshot.clone(),
                    handle: None,
                },
            );
        }

        let runtime = Arc::clone(self);
        let run_id_for_task = run_id.clone();
        let task_handle = tokio::spawn(async move {
            let result = runtime
                .provider
                .chat_with_system(
                    Some(&runtime.system_prompt),
                    &task,
                    &model_name,
                    runtime.default_temperature,
                )
                .await;
            runtime.complete_run(&run_id_for_task, result);
        });

        {
            let mut runs = self
                .runs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = runs.get_mut(&run_id) {
                entry.handle = Some(task_handle);
            }
        }

        Ok(snapshot)
    }

    pub fn get(&self, run_id: &str) -> Option<SubagentRunSnapshot> {
        let runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        runs.get(run_id).map(|entry| entry.snapshot.clone())
    }

    pub fn list(&self) -> Vec<SubagentRunSnapshot> {
        let runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut snapshots = runs
            .values()
            .map(|entry| entry.snapshot.clone())
            .collect::<Vec<_>>();
        snapshots.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        snapshots
    }

    pub fn cancel(&self, run_id: &str) -> Result<()> {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = runs.get_mut(run_id) else {
            bail!("subagent run not found: {run_id}");
        };
        if entry.snapshot.status != SubagentRunStatus::Running {
            return Ok(());
        }
        if let Some(handle) = entry.handle.take() {
            handle.abort();
        }
        entry.snapshot.status = SubagentRunStatus::Cancelled;
        entry.snapshot.finished_at = Some(Utc::now().to_rfc3339());
        entry.snapshot.output = None;
        entry.snapshot.error = Some("cancelled".to_string());
        Ok(())
    }

    fn complete_run(&self, run_id: &str, result: Result<String>) {
        let mut runs = self
            .runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = runs.get_mut(run_id) {
            entry.snapshot.finished_at = Some(Utc::now().to_rfc3339());
            entry.handle = None;
            match result {
                Ok(output) => {
                    entry.snapshot.status = SubagentRunStatus::Completed;
                    entry.snapshot.output = Some(output);
                    entry.snapshot.error = None;
                }
                Err(error) => {
                    entry.snapshot.status = SubagentRunStatus::Failed;
                    entry.snapshot.output = None;
                    entry.snapshot.error = Some(error.to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;

    struct TestProvider;

    impl Provider for TestProvider {
        fn name(&self) -> &str {
            "test"
        }

        fn chat_with_system<'a>(
            &'a self,
            _system_prompt: Option<&'a str>,
            message: &'a str,
            _model: &'a str,
            _temperature: f64,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
            Box::pin(async move { Ok(format!("echo:{message}")) })
        }
    }

    #[tokio::test]
    async fn runtime_run_inline() {
        let runtime = SubagentRuntime::new(Arc::new(TestProvider), "system", "test-model", 0.5);
        let result = runtime.run_inline("hello".into(), None).await.unwrap();
        assert_eq!(result, "echo:hello");
    }

    #[tokio::test]
    async fn runtime_spawn_and_get() {
        let runtime = Arc::new(SubagentRuntime::new(
            Arc::new(TestProvider),
            "system",
            "test-model",
            0.5,
        ));

        let snapshot = runtime
            .spawn("test-task".into(), Some("label-a".into()), None)
            .unwrap();
        assert!(snapshot.run_id.starts_with("subagent_"));
        assert_eq!(snapshot.label.as_deref(), Some("label-a"));
        assert_eq!(snapshot.status, SubagentRunStatus::Running);

        // Wait for completion
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let completed = runtime.get(&snapshot.run_id).unwrap();
        assert_eq!(completed.status, SubagentRunStatus::Completed);
        assert_eq!(completed.output.as_deref(), Some("echo:test-task"));
    }

    #[tokio::test]
    async fn runtime_list_and_cancel() {
        let runtime = Arc::new(SubagentRuntime::new(
            Arc::new(TestProvider),
            "system",
            "test-model",
            0.5,
        ));

        let _ = runtime.spawn("task-1".into(), None, None).unwrap();
        let snap2 = runtime.spawn("task-2".into(), None, None).unwrap();

        // List before completion
        let snapshots = runtime.list();
        assert_eq!(snapshots.len(), 2);

        // Cancel a non-existent run
        let err = runtime.cancel("nonexistent");
        assert!(err.is_err());

        // Cancel is idempotent after completion
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        runtime.cancel(&snap2.run_id).unwrap();
    }
}
