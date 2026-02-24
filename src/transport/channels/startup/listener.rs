use crate::config::Config;
use anyhow::Result;
use std::sync::Arc;

use super::super::message_handler::handle_channel_message;
use super::super::runtime::{channel_backoff_settings, spawn_supervised_listener};
use super::super::traits::ChannelMessage;
use super::runtime::init_channel_runtime;

pub async fn start_channels(config: Arc<Config>) -> Result<()> {
    let rt = init_channel_runtime(&config).await?;

    if rt.channels.is_empty() {
        println!("No channels configured. Run `asteroniris onboard` to set up.");
        return Ok(());
    }

    println!("Channel server:");
    println!("  > model: {}", rt.model);
    println!(
        "  > memory: {} (auto-save: {})",
        rt.config.memory.backend,
        if rt.config.memory.auto_save {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  > channels: {}",
        rt.channels
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!("  Listening...");
    println!();

    let (initial_backoff_secs, max_backoff_secs) = channel_backoff_settings(&rt.config.reliability);

    let (tx, mut rx) = tokio::sync::mpsc::channel::<ChannelMessage>(100);

    let mut handles = Vec::with_capacity(rt.channels.len());
    for ch in &rt.channels {
        handles.push(spawn_supervised_listener(
            ch.clone(),
            tx.clone(),
            initial_backoff_secs,
            max_backoff_secs,
        ));
    }
    drop(tx);

    while let Some(msg) = rx.recv().await {
        handle_channel_message(&rt, &msg).await;
    }

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}
