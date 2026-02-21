use anyhow::Result;
use std::sync::Arc;
use tokio::time::Duration;

pub(super) async fn run_heartbeat_worker(config: Arc<crate::config::Config>) -> Result<()> {
    let observer: Arc<dyn crate::observability::Observer> =
        Arc::from(crate::observability::create_observer(&config.observability));
    let engine = crate::diagnostics::heartbeat::engine::HeartbeatEngine::new(
        config.heartbeat.clone(),
        config.workspace_dir.clone(),
        observer,
    );

    let interval_mins = config.heartbeat.interval_minutes.max(5);
    let mut interval = tokio::time::interval(Duration::from_secs(u64::from(interval_mins) * 60));

    loop {
        interval.tick().await;

        let tasks = engine.collect_tasks().await?;
        if tasks.is_empty() {
            continue;
        }

        for task in tasks {
            let prompt = format!("[Heartbeat Task] {task}");
            let temp = config.default_temperature;
            if let Err(e) =
                crate::agent::run(Arc::clone(&config), Some(prompt), None, None, temp).await
            {
                crate::diagnostics::health::mark_component_error("heartbeat", e.to_string());
                tracing::warn!("Heartbeat task failed: {e}");
            } else {
                crate::diagnostics::health::mark_component_ok("heartbeat");
            }
        }
    }
}
