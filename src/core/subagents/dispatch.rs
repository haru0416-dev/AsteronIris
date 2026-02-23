use super::coordination::{AggregatedResult, CoordinationSession, DispatchResult};
use super::{SubagentRunStatus, run_inline};
use super::roles::AgentRole;
use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::time::timeout;
use uuid::Uuid;

pub async fn dispatch_parallel(
    session: &CoordinationSession,
    tasks: Vec<(AgentRole, String)>,
) -> Result<AggregatedResult> {
    let total_start = Instant::now();

    let mut handles = Vec::with_capacity(tasks.len());
    for (role, task) in tasks {
        let role_config = session
            .roles
            .iter()
            .find(|assignment| assignment.role == role)
            .map(|assignment| assignment.config.clone());
        let role_for_join_error = role.clone();

        let handle = tokio::spawn(async move {
            let started_at = Instant::now();
            let run_id = format!("run_{}", Uuid::new_v4());

            let Some(config) = role_config else {
                #[allow(clippy::cast_possible_truncation)]
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                return DispatchResult {
                    run_id,
                    role,
                    status: SubagentRunStatus::Failed,
                    output: None,
                    error: Some("role config not found".to_string()),
                    elapsed_ms,
                };
            };

            let timeout_secs = config.timeout_secs.unwrap_or(60);
            let model_override = config.model_override;
            let dispatch_result = timeout(Duration::from_secs(timeout_secs), run_inline(task, model_override)).await;

            #[allow(clippy::cast_possible_truncation)]
            let elapsed_ms = started_at.elapsed().as_millis() as u64;

            match dispatch_result {
                Ok(Ok(output)) => DispatchResult {
                    run_id,
                    role,
                    status: SubagentRunStatus::Completed,
                    output: Some(output),
                    error: None,
                    elapsed_ms,
                },
                Ok(Err(error)) => DispatchResult {
                    run_id,
                    role,
                    status: SubagentRunStatus::Failed,
                    output: None,
                    error: Some(error.to_string()),
                    elapsed_ms,
                },
                Err(_) => DispatchResult {
                    run_id,
                    role,
                    status: SubagentRunStatus::Cancelled,
                    output: None,
                    error: Some("timeout".to_string()),
                    elapsed_ms,
                },
            }
        });

        handles.push((role_for_join_error, handle));
    }

    let mut results = Vec::with_capacity(handles.len());
    for (role, handle) in handles {
        match handle.await {
            Ok(result) => results.push(result),
            Err(error) => results.push(DispatchResult {
                run_id: format!("run_{}", Uuid::new_v4()),
                role,
                status: SubagentRunStatus::Failed,
                output: None,
                error: Some(error.to_string()),
                elapsed_ms: 0,
            }),
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let total_elapsed_ms = total_start.elapsed().as_millis() as u64;

    let all_succeeded = results
        .iter()
        .all(|result| result.status == SubagentRunStatus::Completed);

    Ok(AggregatedResult {
        session_id: session.session_id.clone(),
        results,
        total_elapsed_ms,
        all_succeeded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::providers::Provider;
    use crate::core::subagents::{SubagentRuntimeConfig, TEST_RUNTIME_LOCK, configure_runtime};
    use crate::core::subagents::roles::{RoleAssignment, RoleConfig};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct DispatchTestProvider;

    #[async_trait]
    impl Provider for DispatchTestProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            if message.contains("fail") {
                anyhow::bail!("forced failure");
            }

            if let Some(ms_text) = message.strip_prefix("sleep:") {
                let millis = ms_text.parse::<u64>().unwrap_or(0);
                tokio::time::sleep(Duration::from_millis(millis)).await;
            }

            Ok(format!("subagent:{message}"))
        }
    }

    fn make_session(session_id: &str, role_configs: Vec<RoleConfig>) -> CoordinationSession {
        let now = chrono::Utc::now().to_rfc3339();
        let roles = role_configs
            .into_iter()
            .map(|config| RoleAssignment {
                run_id: format!("run_{}", Uuid::new_v4()),
                role: config.role.clone(),
                config,
                assigned_at: now.clone(),
            })
            .collect();

        CoordinationSession {
            session_id: session_id.to_string(),
            roles,
            shared_context: super::super::coordination::SharedContext::default(),
            created_at: now,
        }
    }

    fn role_config(role: AgentRole, timeout_secs: Option<u64>) -> RoleConfig {
        RoleConfig {
            role,
            system_prompt_override: None,
            model_override: None,
            temperature_override: None,
            timeout_secs,
        }
    }

    fn configure_test_runtime() {
        configure_runtime(SubagentRuntimeConfig {
            provider: Arc::new(DispatchTestProvider),
            system_prompt: "sys".to_string(),
            default_model: "test-model".to_string(),
            default_temperature: 0.0,
        })
        .expect("runtime config should succeed");
    }

    #[tokio::test]
    async fn dispatch_parallel_all_success() {
        let _guard = TEST_RUNTIME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        configure_test_runtime();

        let session = make_session(
            "coord_success",
            vec![
                role_config(AgentRole::Planner, Some(5)),
                role_config(AgentRole::Executor, Some(5)),
            ],
        );
        let tasks = vec![
            (AgentRole::Planner, "task-a".to_string()),
            (AgentRole::Executor, "task-b".to_string()),
        ];

        let aggregated = dispatch_parallel(&session, tasks).await.unwrap();

        assert_eq!(aggregated.session_id, "coord_success");
        assert_eq!(aggregated.results.len(), 2);
        assert_eq!(aggregated.results[0].role, AgentRole::Planner);
        assert_eq!(aggregated.results[1].role, AgentRole::Executor);
        assert_eq!(aggregated.results[0].status, SubagentRunStatus::Completed);
        assert_eq!(aggregated.results[1].status, SubagentRunStatus::Completed);
        assert_eq!(aggregated.results[0].output.as_deref(), Some("subagent:task-a"));
        assert_eq!(aggregated.results[1].output.as_deref(), Some("subagent:task-b"));
        assert!(aggregated
            .results
            .iter()
            .all(|result| result.run_id.starts_with("run_")));
        assert!(aggregated.all_succeeded);
    }

    #[tokio::test]
    async fn dispatch_parallel_partial_failure() {
        let _guard = TEST_RUNTIME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        configure_test_runtime();

        let session = make_session(
            "coord_partial",
            vec![
                role_config(AgentRole::Planner, Some(5)),
                role_config(AgentRole::Executor, Some(5)),
            ],
        );
        let tasks = vec![
            (AgentRole::Planner, "task-ok".to_string()),
            (AgentRole::Executor, "fail-task".to_string()),
        ];

        let aggregated = dispatch_parallel(&session, tasks).await.unwrap();

        assert_eq!(aggregated.results.len(), 2);
        assert_eq!(aggregated.results[0].status, SubagentRunStatus::Completed);
        assert_eq!(aggregated.results[1].status, SubagentRunStatus::Failed);
        assert_eq!(aggregated.results[1].output, None);
        assert!(aggregated.results[1]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("forced failure")));
        assert!(!aggregated.all_succeeded);
    }

    #[tokio::test]
    async fn dispatch_parallel_timeout() {
        let _guard = TEST_RUNTIME_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        configure_test_runtime();

        let session = make_session(
            "coord_timeout",
            vec![role_config(AgentRole::Reviewer, Some(1))],
        );
        let tasks = vec![(AgentRole::Reviewer, "sleep:1500".to_string())];

        let aggregated = dispatch_parallel(&session, tasks).await.unwrap();

        assert_eq!(aggregated.results.len(), 1);
        assert_eq!(aggregated.results[0].status, SubagentRunStatus::Cancelled);
        assert_eq!(aggregated.results[0].output, None);
        assert_eq!(aggregated.results[0].error.as_deref(), Some("timeout"));
        assert!(!aggregated.all_succeeded);
    }
}
