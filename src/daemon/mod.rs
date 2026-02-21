use crate::config::Config;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;

mod heartbeat_worker;
mod state;
mod supervisor;

use state::spawn_state_writer;
use supervisor::spawn_supervised_components;

const STATUS_FLUSH_SECONDS: u64 = 5;

pub async fn run(config: Arc<Config>, host: String, port: u16) -> Result<()> {
    let initial_backoff = config.reliability.channel_initial_backoff_secs.max(1);
    let max_backoff = config
        .reliability
        .channel_max_backoff_secs
        .max(initial_backoff);

    crate::diagnostics::health::mark_component_ok("daemon");

    if config.heartbeat.enabled {
        let _ = crate::diagnostics::heartbeat::engine::HeartbeatEngine::ensure_heartbeat_file(
            &config.workspace_dir,
        )
        .await;
    }

    let mut handles: Vec<JoinHandle<()>> = vec![spawn_state_writer(Arc::clone(&config))];
    handles.extend(spawn_supervised_components(
        Arc::clone(&config),
        host.clone(),
        port,
        initial_backoff,
        max_backoff,
        has_supervised_channels(&config),
    ));

    println!("◆ {}", t!("daemon.started"));
    println!("   {}", t!("daemon.gateway_addr", host = host, port = port));
    println!("   {}", t!("daemon.components"));
    println!("   {}", t!("daemon.stop_hint"));

    tokio::signal::ctrl_c().await?;
    crate::diagnostics::health::mark_component_error("daemon", "shutdown requested");

    for handle in &handles {
        handle.abort();
    }
    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}

pub fn state_file_path(config: &Config) -> PathBuf {
    state::state_file_path(config)
}

fn has_supervised_channels(config: &Config) -> bool {
    config.channels_config.telegram.is_some()
        || config.channels_config.discord.is_some()
        || config.channels_config.slack.is_some()
        || config.channels_config.imessage.is_some()
        || config.channels_config.matrix.is_some()
        || config.channels_config.whatsapp.is_some()
        || config.channels_config.email.is_some()
}
