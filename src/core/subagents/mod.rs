use crate::core::providers::Provider;
use anyhow::{Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub mod coordination;
pub mod dispatch;
pub mod roles;

#[derive(Clone)]
pub struct SubagentRuntimeConfig {
    pub provider: Arc<dyn Provider>,
    pub system_prompt: String,
    pub default_model: String,
    pub default_temperature: f64,
}

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

static RUNTIME: OnceLock<RwLock<Option<SubagentRuntimeConfig>>> = OnceLock::new();
static RUNS: OnceLock<Mutex<HashMap<String, SubagentRunEntry>>> = OnceLock::new();

fn runtime_lock() -> &'static RwLock<Option<SubagentRuntimeConfig>> {
    RUNTIME.get_or_init(|| RwLock::new(None))
}

fn runs_lock() -> &'static Mutex<HashMap<String, SubagentRunEntry>> {
    RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_runtime() -> Result<SubagentRuntimeConfig> {
    let guard = runtime_lock()
        .read()
        .map_err(|error| anyhow::anyhow!("subagent runtime lock poisoned: {error}"))?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("subagent runtime is not configured"))
}

fn complete_run(run_id: &str, result: Result<String>) {
    let mut runs = runs_lock()
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

pub fn configure_runtime(config: SubagentRuntimeConfig) -> Result<()> {
    let mut guard = runtime_lock()
        .write()
        .map_err(|error| anyhow::anyhow!("subagent runtime lock poisoned: {error}"))?;
    *guard = Some(config);
    Ok(())
}

pub async fn run_inline(task: String, model: Option<String>) -> Result<String> {
    let runtime = get_runtime()?;
    let model_name = model.unwrap_or(runtime.default_model);
    runtime
        .provider
        .chat_with_system(
            Some(runtime.system_prompt.as_str()),
            &task,
            &model_name,
            runtime.default_temperature,
        )
        .await
}

pub fn spawn(
    task: String,
    label: Option<String>,
    model: Option<String>,
) -> Result<SubagentRunSnapshot> {
    let runtime = get_runtime()?;
    let run_id = format!("subagent_{}", Uuid::new_v4().simple());
    let model_name = model.unwrap_or(runtime.default_model);
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
        let mut runs = runs_lock()
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

    let run_id_for_task = run_id.clone();
    let task_handle = tokio::spawn(async move {
        let result = runtime
            .provider
            .chat_with_system(
                Some(runtime.system_prompt.as_str()),
                &task,
                &model_name,
                runtime.default_temperature,
            )
            .await;
        complete_run(&run_id_for_task, result);
    });

    {
        let mut runs = runs_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = runs.get_mut(&run_id) {
            entry.handle = Some(task_handle);
        }
    }

    Ok(snapshot)
}

pub fn get(run_id: &str) -> Option<SubagentRunSnapshot> {
    let runs = runs_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    runs.get(run_id).map(|entry| entry.snapshot.clone())
}

pub fn list() -> Vec<SubagentRunSnapshot> {
    let runs = runs_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut snapshots = runs
        .values()
        .map(|entry| entry.snapshot.clone())
        .collect::<Vec<_>>();
    snapshots.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    snapshots
}

pub fn cancel(run_id: &str) -> Result<()> {
    let mut runs = runs_lock()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::providers::Provider;
    use async_trait::async_trait;

    struct MockProvider;

    #[async_trait]
    impl Provider for MockProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(format!("subagent:{message}"))
        }
    }

    #[tokio::test]
    async fn subagent_inline_and_background_runs_complete() {
        configure_runtime(SubagentRuntimeConfig {
            provider: Arc::new(MockProvider),
            system_prompt: "sys".to_string(),
            default_model: "test-model".to_string(),
            default_temperature: 0.0,
        })
        .unwrap();

        let inline = run_inline("hello".to_string(), None).await.unwrap();
        assert_eq!(inline, "subagent:hello");

        let started = spawn("world".to_string(), Some("bg".to_string()), None).unwrap();
        assert_eq!(started.status, SubagentRunStatus::Running);
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        let done = get(&started.run_id).unwrap();
        assert_eq!(done.status, SubagentRunStatus::Completed);
        assert_eq!(done.output.as_deref(), Some("subagent:world"));
    }

    #[tokio::test]
    async fn subagent_list_and_cancel_work() {
        configure_runtime(SubagentRuntimeConfig {
            provider: Arc::new(MockProvider),
            system_prompt: "sys".to_string(),
            default_model: "test-model".to_string(),
            default_temperature: 0.0,
        })
        .unwrap();

        let started = spawn("cancel-me".to_string(), Some("bg".to_string()), None).unwrap();
        let listed = list();
        assert!(listed.iter().any(|item| item.run_id == started.run_id));

        cancel(&started.run_id).unwrap();
        let cancelled = get(&started.run_id).unwrap();
        assert_eq!(cancelled.status, SubagentRunStatus::Cancelled);
    }
}
