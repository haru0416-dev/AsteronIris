use crate::config::Config;
use anyhow::Result;
use std::future::Future;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio::time::Duration;

pub(super) fn spawn_component_supervisor<F, Fut>(
    name: &'static str,
    initial_backoff_secs: u64,
    max_backoff_secs: u64,
    max_restarts: u32,
    mut run_component: F,
) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut backoff = initial_backoff_secs.max(1);
        let max_backoff = max_backoff_secs.max(backoff);
        let mut consecutive_failures: u32 = 0;

        loop {
            tracing::info!("Daemon component '{name}' starting");
            match run_component().await {
                Ok(()) => {
                    tracing::warn!("Daemon component '{name}' exited unexpectedly");
                    backoff = initial_backoff_secs.max(1);
                    consecutive_failures = consecutive_failures.saturating_add(1);
                }
                Err(e) => {
                    tracing::error!("Daemon component '{name}' failed: {e}");
                    consecutive_failures = consecutive_failures.saturating_add(1);
                }
            }

            if max_restarts > 0 && consecutive_failures > max_restarts {
                tracing::error!(
                    "Daemon component '{name}' exceeded max restarts ({max_restarts}), circuit open"
                );
                break;
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
            backoff = backoff.saturating_mul(2).min(max_backoff);
        }
    })
}

pub(super) fn spawn_supervised_components(
    config: Arc<Config>,
    host: String,
    port: u16,
    initial_backoff: u64,
    max_backoff: u64,
    supervise_channels: bool,
) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::new();

    let gateway_cfg = Arc::clone(&config);
    handles.push(spawn_component_supervisor(
        "gateway",
        initial_backoff,
        max_backoff,
        10,
        move || {
            let cfg = Arc::clone(&gateway_cfg);
            let host = host.clone();
            async move { crate::transport::gateway::run_gateway(&host, port, cfg).await }
        },
    ));

    if supervise_channels {
        let channels_cfg = Arc::clone(&config);
        handles.push(spawn_component_supervisor(
            "channels",
            initial_backoff,
            max_backoff,
            10,
            move || {
                let cfg = Arc::clone(&channels_cfg);
                async move { crate::transport::channels::start_channels(cfg).await }
            },
        ));
    } else {
        tracing::info!("No real-time channels configured; channel supervisor disabled");
    }

    if config.heartbeat.enabled {
        let heartbeat_cfg = Arc::clone(&config);
        handles.push(spawn_component_supervisor(
            "heartbeat",
            initial_backoff,
            max_backoff,
            10,
            move || {
                let cfg = Arc::clone(&heartbeat_cfg);
                async move { super::heartbeat_worker::run_heartbeat_worker(cfg).await }
            },
        ));
    }

    let scheduler_cfg = config;
    handles.push(spawn_component_supervisor(
        "scheduler",
        initial_backoff,
        max_backoff,
        10,
        move || {
            let cfg = Arc::clone(&scheduler_cfg);
            async move { crate::platform::cron::scheduler::run(cfg).await }
        },
    ));

    handles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn supervisor_restarts_on_failure() {
        let handle = spawn_component_supervisor("daemon-test-fail", 1, 1, 0, || async {
            anyhow::bail!("boom")
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn supervisor_handles_unexpected_exit() {
        let handle = spawn_component_supervisor("daemon-test-exit", 1, 1, 0, || async { Ok(()) });

        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
        let _ = handle.await;
    }
}
