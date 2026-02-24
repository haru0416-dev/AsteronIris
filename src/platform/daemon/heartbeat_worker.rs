use crate::agent::tool_loop::{ToolLoop, ToolLoopRunParams};
use crate::config::Config;
use crate::llm::manager::LlmManager;
use crate::memory::factory::create_memory;
use crate::security::SecurityPolicy;
use crate::tools::{ExecutionContext, ToolRegistry, all_tools};
use anyhow::Result;
use arc_swap::ArcSwap;
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

fn heartbeat_temperature(config: &Config) -> f64 {
    config
        .autonomy
        .clamp_temperature(config.default_temperature)
}

pub(super) async fn run_heartbeat_worker(config: Arc<Config>) -> Result<()> {
    let interval_mins = config.heartbeat.interval_minutes.max(5);
    let mut interval = tokio::time::interval(Duration::from_secs(u64::from(interval_mins) * 60));

    // Build shared dependencies for task execution.
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let memory = Arc::from(create_memory(&config.memory, &config.workspace_dir, None).await?);
    let mut registry = ToolRegistry::new(vec![]);
    for tool in all_tools(Arc::clone(&memory)) {
        registry.register(tool);
    }
    let registry = Arc::new(registry);
    let llm_config = Arc::new(ArcSwap::new(Arc::clone(&config)));
    let llm = Arc::new(LlmManager::new(llm_config));

    loop {
        interval.tick().await;
        run_memory_hygiene_tick(&config);
        record_autonomy_mode_transition(&config);

        // Collect heartbeat tasks (simple file-based task list).
        let tasks = collect_heartbeat_tasks(&config).await;
        if tasks.is_empty() {
            continue;
        }

        for task in tasks {
            let prompt = format!("[Heartbeat Task] {task}");
            let temp = heartbeat_temperature(&config);

            let ctx = ExecutionContext::from_security(Arc::clone(&security));
            let tool_loop = ToolLoop::new(Arc::clone(&registry), 10);
            let provider = match llm.get_provider() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Heartbeat task failed to get provider: {e}");
                    continue;
                }
            };

            let model = config
                .default_model
                .as_deref()
                .unwrap_or("claude-sonnet-4-20250514");
            let params = ToolLoopRunParams {
                provider: provider.as_ref(),
                system_prompt: "You are a background maintenance agent. Execute the requested task.",
                user_message: &prompt,
                image_content: &[],
                model,
                temperature: temp,
                ctx: &ctx,
                stream_sink: None,
                conversation_history: &[],
                hooks: &[],
            };

            match tool_loop.run(params).await {
                Ok(_result) => {
                    tracing::info!("Heartbeat task completed: {task}");
                }
                Err(e) => {
                    tracing::warn!("Heartbeat task failed: {e}");
                }
            }
        }
    }
}

/// Collect pending heartbeat tasks from the workspace heartbeat file.
///
/// Returns an empty vec if the heartbeat file doesn't exist or has no tasks.
async fn collect_heartbeat_tasks(config: &Config) -> Vec<String> {
    let heartbeat_dir = config.workspace_dir.join("heartbeat");
    let tasks_file = heartbeat_dir.join("pending_tasks.txt");

    let Ok(content) = tokio::fs::read_to_string(&tasks_file).await else {
        return Vec::new();
    };

    let tasks: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect();

    // Clear the file after collecting tasks.
    if !tasks.is_empty() {
        let _ = tokio::fs::write(&tasks_file, "").await;
    }

    tasks
}

#[cfg(test)]
mod tests;
