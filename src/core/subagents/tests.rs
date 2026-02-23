use super::*;
use crate::core::providers::Provider;
use std::future::Future;
use std::pin::Pin;

struct MockProvider;

impl Provider for MockProvider {
    fn chat_with_system<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        message: &'a str,
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move { Ok(format!("subagent:{message}")) })
    }
}

#[tokio::test]
async fn subagent_inline_and_background_runs_complete() {
    let _guard = TEST_RUNTIME_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

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
    let _guard = TEST_RUNTIME_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

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
