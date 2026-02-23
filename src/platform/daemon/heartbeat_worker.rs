use anyhow::Result;
use std::sync::Arc;
use tokio::time::Duration;

mod autonomy_state;
mod memory_metrics;

use autonomy_state::record_autonomy_mode_transition;
use memory_metrics::run_memory_hygiene_tick;
#[cfg(test)]
use memory_metrics::{
    belief_promotion_total, contradiction_mark_total, contradiction_ratio, evaluate_memory_slo,
    stale_trend_purge_total,
};

fn heartbeat_temperature(config: &crate::config::Config) -> f64 {
    config
        .autonomy
        .clamp_temperature(config.default_temperature)
}

pub(super) async fn run_heartbeat_worker(config: Arc<crate::config::Config>) -> Result<()> {
    let observer: Arc<dyn crate::runtime::observability::Observer> = Arc::from(
        crate::runtime::observability::create_observer(&config.observability),
    );
    let engine = crate::runtime::diagnostics::heartbeat::engine::HeartbeatEngine::new(
        config.heartbeat.clone(),
        config.workspace_dir.clone(),
        Arc::clone(&observer),
    );

    let interval_mins = config.heartbeat.interval_minutes.max(5);
    let mut interval = tokio::time::interval(Duration::from_secs(u64::from(interval_mins) * 60));

    loop {
        interval.tick().await;
        run_memory_hygiene_tick(&config, &observer);
        record_autonomy_mode_transition(&config, &observer);

        let tasks = engine.collect_tasks().await?;
        if tasks.is_empty() {
            continue;
        }

        for task in tasks {
            let prompt = format!("[Heartbeat Task] {task}");
            let temp = heartbeat_temperature(&config);
            if let Err(e) =
                crate::core::agent::run(Arc::clone(&config), Some(prompt), None, None, temp).await
            {
                crate::runtime::diagnostics::health::mark_component_error(
                    "heartbeat",
                    e.to_string(),
                );
                tracing::warn!("Heartbeat task failed: {e}");
            } else {
                crate::runtime::diagnostics::health::mark_component_ok("heartbeat");
            }
        }
    }
}

#[cfg(test)]
mod tests;
