use crate::auth::AuthBroker;
use crate::config::Config;
use crate::memory::{self, Memory};
use crate::providers::{self, Provider};
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use super::factory;
use super::health::{ChannelHealthState, classify_health_result};
use super::ingress_policy::{
    apply_external_ingress_policy, channel_autosave_entity_id, channel_autosave_input,
    channel_runtime_policy_context,
};
use super::prompt_builder::build_system_prompt;
use super::runtime::{channel_backoff_settings, spawn_supervised_listener};
use super::traits::{Channel, ChannelMessage};

struct ChannelRuntime {
    config: Arc<Config>,
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    mem: Arc<dyn Memory>,
    system_prompt: String,
    channels: Vec<Arc<dyn Channel>>,
}

pub async fn doctor_channels(config: Arc<Config>) -> Result<()> {
    let channels = factory::build_channels(&config.channels_config);

    if channels.is_empty() {
        println!("{}", t!("channels.no_channels_doctor"));
        return Ok(());
    }

    println!("◆ {}", t!("channels.doctor_title"));
    println!();

    let mut healthy = 0_u32;
    let mut unhealthy = 0_u32;
    let mut timeout = 0_u32;

    for (name, channel) in channels {
        let result = tokio::time::timeout(Duration::from_secs(10), channel.health_check()).await;
        let state = classify_health_result(&result);

        match state {
            ChannelHealthState::Healthy => {
                healthy += 1;
                println!("  ✓ {name:<9} {}", t!("channels.healthy"));
            }
            ChannelHealthState::Unhealthy => {
                unhealthy += 1;
                println!("  ✗ {name:<9} {}", t!("channels.unhealthy"));
            }
            ChannelHealthState::Timeout => {
                timeout += 1;
                println!("  ! {name:<9} {}", t!("channels.timed_out"));
            }
        }
    }

    if config.channels_config.webhook.is_some() {
        println!("  › {}", t!("channels.webhook_hint"));
    }

    println!();
    println!(
        "{}",
        t!(
            "channels.doctor_summary",
            healthy = healthy,
            unhealthy = unhealthy,
            timeout = timeout
        )
    );
    Ok(())
}

async fn init_channel_runtime(config: &Arc<Config>) -> Result<ChannelRuntime> {
    let auth_broker = AuthBroker::load_or_init(config)?;

    let provider: Arc<dyn Provider> =
        Arc::from(providers::create_resilient_provider_with_oauth_recovery(
            config,
            config.default_provider.as_deref().unwrap_or("openrouter"),
            &config.reliability,
            |name| auth_broker.resolve_provider_api_key(name),
        )?);

    if let Err(e) = provider.warmup().await {
        tracing::warn!("Provider warmup failed (non-fatal): {e}");
    }

    let model = config
        .default_model
        .clone()
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
    let temperature = config.default_temperature;
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        memory_api_key.as_deref(),
    )?);

    let workspace = config.workspace_dir.clone();
    let skills = crate::skills::load_skills(&workspace);
    let tool_descs = crate::tools::tool_descriptions(config.browser.enabled, false);
    let system_prompt = build_system_prompt(&workspace, &model, &tool_descs, &skills);

    if !skills.is_empty() {
        println!(
            "  › {} {}",
            t!("channels.skills"),
            skills
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let channels: Vec<Arc<dyn Channel>> = factory::build_channels(&config.channels_config)
        .into_iter()
        .map(|(_, ch)| ch)
        .collect();

    Ok(ChannelRuntime {
        config: Arc::clone(config),
        provider,
        model,
        temperature,
        mem,
        system_prompt,
        channels,
    })
}

async fn reply_to_origin(
    channels: &[Arc<dyn Channel>],
    channel_name: &str,
    message: &str,
    sender: &str,
) -> Result<()> {
    for ch in channels {
        if ch.name() == channel_name {
            ch.send_chunked(message, sender).await?;
            break;
        }
    }
    Ok(())
}

async fn handle_channel_message(rt: &ChannelRuntime, msg: &ChannelMessage) {
    println!(
        "  › {}",
        t!(
            "channels.message_in",
            channel = msg.channel,
            sender = msg.sender,
            content = truncate_with_ellipsis(&msg.content, 80)
        )
    );

    let source = format!("channel:{}", msg.channel);
    let ingress = apply_external_ingress_policy(&source, &msg.content);

    if rt.config.memory.auto_save {
        let policy_context = channel_runtime_policy_context();
        if let Err(error) = policy_context.enforce_recall_scope(channel_autosave_entity_id()) {
            tracing::warn!(error, "channel autosave skipped due to policy context");
        } else {
            let _ = rt
                .mem
                .append_event(channel_autosave_input(
                    &msg.channel,
                    &msg.sender,
                    ingress.persisted_summary.clone(),
                ))
                .await;
        }
    }

    if ingress.blocked {
        tracing::warn!(
            source,
            "blocked high-risk external content at channel ingress"
        );
        let _ = reply_to_origin(
            &rt.channels,
            &msg.channel,
            "⚠️ External content was blocked by safety policy.",
            &msg.sender,
        )
        .await;
        return;
    }

    match rt
        .provider
        .chat_with_system(
            Some(&rt.system_prompt),
            &ingress.model_input,
            &rt.model,
            rt.temperature,
        )
        .await
    {
        Ok(response) => {
            println!(
                "  › {} {}",
                t!("channels.reply"),
                truncate_with_ellipsis(&response, 80)
            );
            if let Err(e) =
                reply_to_origin(&rt.channels, &msg.channel, &response, &msg.sender).await
            {
                eprintln!(
                    "  ✗ {}",
                    t!("channels.reply_fail", channel = msg.channel, error = e)
                );
            }
        }
        Err(e) => {
            eprintln!("  ✗ {}", t!("channels.llm_error", error = e));
            let _ = reply_to_origin(
                &rt.channels,
                &msg.channel,
                &format!("! Error: {e}"),
                &msg.sender,
            )
            .await;
        }
    }
}

pub async fn start_channels(config: Arc<Config>) -> Result<()> {
    let rt = init_channel_runtime(&config).await?;

    if rt.channels.is_empty() {
        println!("{}", t!("channels.no_channels"));
        return Ok(());
    }

    println!("◆ {}", t!("channels.server_title"));
    println!("  › {} {}", t!("channels.model"), rt.model);
    println!(
        "  › {} {} (auto-save: {})",
        t!("channels.memory"),
        rt.config.memory.backend,
        if rt.config.memory.auto_save {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  › {} {}",
        t!("channels.channels"),
        rt.channels
            .iter()
            .map(|c| c.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!("  {}", t!("channels.listening"));
    println!();

    crate::health::mark_component_ok("channels");

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
