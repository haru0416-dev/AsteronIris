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
    mut run_component: F,
) -> JoinHandle<()>
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut backoff = initial_backoff_secs.max(1);
        let max_backoff = max_backoff_secs.max(backoff);

        loop {
            crate::diagnostics::health::mark_component_ok(name);
            match run_component().await {
                Ok(()) => {
                    crate::diagnostics::health::mark_component_error(
                        name,
                        "component exited unexpectedly",
                    );
                    tracing::warn!("Daemon component '{name}' exited unexpectedly");
                    backoff = initial_backoff_secs.max(1);
                }
                Err(e) => {
                    crate::diagnostics::health::mark_component_error(name, e.to_string());
                    tracing::error!("Daemon component '{name}' failed: {e}");
                }
            }

            crate::diagnostics::health::bump_component_restart(name);
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
        move || {
            let cfg = Arc::clone(&gateway_cfg);
            let host = host.clone();
            async move { crate::gateway::run_gateway(&host, port, cfg).await }
        },
    ));

    if supervise_channels {
        let channels_cfg = Arc::clone(&config);
        handles.push(spawn_component_supervisor(
            "channels",
            initial_backoff,
            max_backoff,
            move || {
                let cfg = Arc::clone(&channels_cfg);
                async move { crate::channels::start_channels(cfg).await }
            },
        ));
    } else {
        crate::diagnostics::health::mark_component_ok("channels");
        tracing::info!("No real-time channels configured; channel supervisor disabled");
    }

    if config.heartbeat.enabled {
        let heartbeat_cfg = Arc::clone(&config);
        handles.push(spawn_component_supervisor(
            "heartbeat",
            initial_backoff,
            max_backoff,
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
    async fn supervisor_marks_error_and_restart_on_failure() {
        let handle = spawn_component_supervisor("daemon-test-fail", 1, 1, || async {
            anyhow::bail!("boom")
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
        let _ = handle.await;

        let snapshot = crate::diagnostics::health::snapshot_json();
        let component = &snapshot["components"]["daemon-test-fail"];
        assert_eq!(component["status"], "error");
        assert!(component["restart_count"].as_u64().unwrap_or(0) >= 1);
        assert!(
            component["last_error"]
                .as_str()
                .unwrap_or("")
                .contains("boom")
        );
    }

    #[tokio::test]
    async fn supervisor_marks_unexpected_exit_as_error() {
        let handle = spawn_component_supervisor("daemon-test-exit", 1, 1, || async { Ok(()) });

        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
        let _ = handle.await;

        let snapshot = crate::diagnostics::health::snapshot_json();
        let component = &snapshot["components"]["daemon-test-exit"];
        assert_eq!(component["status"], "error");
        assert!(component["restart_count"].as_u64().unwrap_or(0) >= 1);
        assert!(
            component["last_error"]
                .as_str()
                .unwrap_or("")
                .contains("component exited unexpectedly")
        );
    }
}
