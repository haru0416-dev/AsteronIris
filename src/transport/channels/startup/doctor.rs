use crate::config::Config;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use super::super::factory;
use super::super::health::{ChannelHealthState, classify_health_result};

pub async fn doctor_channels(config: Arc<Config>) -> Result<()> {
    let channels = factory::build_channels(config.channels_config.clone());

    if channels.is_empty() {
        println!("No channels configured. Run `asteroniris onboard` to set up channels.");
        return Ok(());
    }

    println!("Channel doctor:");
    println!();

    let mut healthy = 0_u32;
    let mut unhealthy = 0_u32;
    let mut timeout = 0_u32;

    for entry in channels {
        let result =
            tokio::time::timeout(Duration::from_secs(10), entry.channel.health_check()).await;
        let state = classify_health_result(&result);

        match state {
            ChannelHealthState::Healthy => {
                healthy += 1;
                println!("  + {:<9} healthy", entry.name);
            }
            ChannelHealthState::Unhealthy => {
                unhealthy += 1;
                println!("  - {:<9} unhealthy", entry.name);
            }
            ChannelHealthState::Timeout => {
                timeout += 1;
                println!("  ! {:<9} timed out", entry.name);
            }
        }
    }

    if config.channels_config.webhook.is_some() {
        println!("  > Webhook endpoint is configured (check gateway health separately)");
    }

    println!();
    println!("Summary: {healthy} healthy, {unhealthy} unhealthy, {timeout} timed out");
    Ok(())
}
