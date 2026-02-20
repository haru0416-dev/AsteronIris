use crate::agent::tool_loop::{LoopStopReason, ToolLoop};
use crate::auth::AuthBroker;
use crate::config::Config;
use crate::memory::{self, Memory};
use crate::providers::response::ContentBlock;
use crate::providers::{self, Provider};
use crate::security::policy::EntityRateLimiter;
use crate::security::{PermissionStore, SecurityPolicy};
use crate::tools;
use crate::tools::middleware::ExecutionContext;
use crate::tools::registry::ToolRegistry;
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::factory;
use super::health::{ChannelHealthState, classify_health_result};
use super::ingress_policy::{
    apply_external_ingress_policy, channel_autosave_entity_id, channel_autosave_input,
    channel_runtime_policy_context,
};
use super::policy::{ChannelPolicy, min_autonomy};
use super::prompt_builder::build_system_prompt;
use super::runtime::{channel_backoff_settings, spawn_supervised_listener};
use super::traits::{Channel, ChannelMessage};

fn convert_attachments_to_images(
    attachments: &[crate::channels::traits::MediaAttachment],
) -> Vec<ContentBlock> {
    use crate::channels::traits::MediaData;
    use crate::providers::response::ImageSource;

    attachments
        .iter()
        .filter(|a| a.mime_type.starts_with("image/"))
        .map(|a| {
            let source = match &a.data {
                MediaData::Url(url) => ImageSource::url(url),
                MediaData::Bytes(bytes) => ImageSource::base64(&a.mime_type, encode_base64(bytes)),
            };
            ContentBlock::Image { source }
        })
        .collect()
}

fn encode_base64(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(triple >> 18 & 0x3F) as usize] as char);
        out.push(CHARS[(triple >> 12 & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[(triple >> 6 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

struct ChannelRuntime {
    config: Arc<Config>,
    security: Arc<SecurityPolicy>,
    provider: Arc<dyn Provider>,
    registry: Arc<ToolRegistry>,
    rate_limiter: Arc<EntityRateLimiter>,
    permission_store: Arc<PermissionStore>,
    model: String,
    temperature: f64,
    mem: Arc<dyn Memory>,
    system_prompt: String,
    channels: Vec<Arc<dyn Channel>>,
    channel_policies: HashMap<String, ChannelPolicy>,
}

pub async fn doctor_channels(config: Arc<Config>) -> Result<()> {
    let channels = factory::build_channels(config.channels_config.clone());

    if channels.is_empty() {
        println!("{}", t!("channels.no_channels_doctor"));
        return Ok(());
    }

    println!("◆ {}", t!("channels.doctor_title"));
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
                println!("  ✓ {:<9} {}", entry.name, t!("channels.healthy"));
            }
            ChannelHealthState::Unhealthy => {
                unhealthy += 1;
                println!("  ✗ {:<9} {}", entry.name, t!("channels.unhealthy"));
            }
            ChannelHealthState::Timeout => {
                timeout += 1;
                println!("  ! {:<9} {}", entry.name, t!("channels.timed_out"));
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
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let rate_limiter = Arc::new(EntityRateLimiter::new(
        config.autonomy.max_actions_per_hour,
        config.autonomy.max_actions_per_entity_per_hour,
    ));
    let permission_store = Arc::new(PermissionStore::load(&config.workspace_dir));
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        memory_api_key.as_deref(),
    )?);
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools = tools::all_tools(
        &security,
        Arc::clone(&mem),
        composio_key,
        &config.browser,
        &config.tools,
    );
    let middleware = tools::default_middleware_chain();
    let mut registry = ToolRegistry::new(middleware);
    for tool in tools {
        registry.register(tool);
    }

    let workspace = config.workspace_dir.clone();
    let skills = crate::skills::load_skills(&workspace);
    let tool_descs =
        crate::tools::tool_descriptions(config.browser.enabled, config.composio.enabled);
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

    let mut channels: Vec<Arc<dyn Channel>> = Vec::new();
    let mut channel_policies = HashMap::new();
    for entry in factory::build_channels(config.channels_config.clone()) {
        channel_policies.insert(entry.channel.name().to_string(), entry.policy);
        channels.push(entry.channel);
    }

    Ok(ChannelRuntime {
        config: Arc::clone(config),
        security,
        provider,
        registry: Arc::new(registry),
        rate_limiter,
        permission_store,
        model,
        temperature,
        mem,
        system_prompt,
        channels,
        channel_policies,
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

#[allow(clippy::too_many_lines)]
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

    let global_autonomy = rt.config.autonomy.level;
    let channel_policy = rt.channel_policies.get(&msg.channel);
    let channel_level = channel_policy
        .and_then(|policy| policy.autonomy_level)
        .unwrap_or(global_autonomy);
    let effective_autonomy = min_autonomy(global_autonomy, channel_level);
    let tool_allowlist = channel_policy.and_then(|policy| policy.tool_allowlist.clone());

    tracing::debug!(
        channel = %msg.channel,
        sender = %msg.sender,
        effective_autonomy = ?effective_autonomy,
        has_tool_allowlist = tool_allowlist.is_some(),
        "resolved channel runtime policy"
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

    let tenant_context = channel_runtime_policy_context();
    let workspace_dir = if tenant_context.tenant_mode_enabled {
        let tenant_id = tenant_context.tenant_id.as_deref().unwrap_or("default");
        let scoped = rt.config.workspace_dir.join("tenants").join(tenant_id);
        if let Err(error) = tokio::fs::create_dir_all(&scoped).await {
            tracing::warn!(
                error = %error,
                tenant_id,
                "failed to create tenant scoped workspace"
            );
        }
        scoped
    } else {
        rt.config.workspace_dir.clone()
    };
    let ctx = ExecutionContext {
        security: Arc::clone(&rt.security),
        autonomy_level: effective_autonomy,
        entity_id: format!("{}:{}", msg.channel, msg.sender),
        turn_number: 0,
        workspace_dir,
        allowed_tools: tool_allowlist,
        permission_store: Some(Arc::clone(&rt.permission_store)),
        rate_limiter: Arc::clone(&rt.rate_limiter),
        tenant_context,
    };
    let tool_loop = ToolLoop::new(
        Arc::clone(&rt.registry),
        rt.config.autonomy.max_tool_loop_iterations,
    );
    let image_blocks = convert_attachments_to_images(&msg.attachments);

    match tool_loop
        .run(
            rt.provider.as_ref(),
            &rt.system_prompt,
            &ingress.model_input,
            &image_blocks,
            &rt.model,
            rt.temperature,
            &ctx,
        )
        .await
    {
        Ok(result) => {
            if let LoopStopReason::Error(error) = &result.stop_reason {
                eprintln!("  ✗ {}", t!("channels.llm_error", error = error));
                let _ = reply_to_origin(
                    &rt.channels,
                    &msg.channel,
                    &format!("! Error: {error}"),
                    &msg.sender,
                )
                .await;
                return;
            }
            match result.stop_reason {
                LoopStopReason::MaxIterations => {
                    tracing::warn!(channel = %msg.channel, sender = %msg.sender, "tool loop hit max iterations");
                }
                LoopStopReason::RateLimited => {
                    tracing::warn!(channel = %msg.channel, sender = %msg.sender, "tool loop halted by rate limiting");
                }
                LoopStopReason::Completed | LoopStopReason::ApprovalDenied => {}
                LoopStopReason::Error(_) => unreachable!("error stop reason handled above"),
            }
            println!(
                "  › {} {}",
                t!("channels.reply"),
                truncate_with_ellipsis(&result.final_text, 80)
            );
            if let Err(e) =
                reply_to_origin(&rt.channels, &msg.channel, &result.final_text, &msg.sender).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::traits::{MediaAttachment, MediaData};

    #[test]
    fn encode_base64_empty() {
        assert_eq!(encode_base64(&[]), "");
    }

    #[test]
    fn encode_base64_hello() {
        assert_eq!(encode_base64(b"Hello"), "SGVsbG8=");
    }

    #[test]
    fn encode_base64_three_byte_aligned() {
        assert_eq!(encode_base64(b"abc"), "YWJj");
    }

    #[test]
    fn convert_attachments_filters_non_images() {
        let attachments = vec![
            MediaAttachment {
                mime_type: "image/png".to_string(),
                data: MediaData::Url("https://example.com/img.png".to_string()),
                filename: Some("img.png".to_string()),
            },
            MediaAttachment {
                mime_type: "audio/mpeg".to_string(),
                data: MediaData::Url("https://example.com/audio.mp3".to_string()),
                filename: Some("audio.mp3".to_string()),
            },
        ];
        let blocks = convert_attachments_to_images(&attachments);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], ContentBlock::Image { .. }));
    }

    #[test]
    fn convert_attachments_url_source() {
        let attachments = vec![MediaAttachment {
            mime_type: "image/jpeg".to_string(),
            data: MediaData::Url("https://example.com/photo.jpg".to_string()),
            filename: None,
        }];
        let blocks = convert_attachments_to_images(&attachments);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Image { source } = &blocks[0] {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "url");
            assert_eq!(json["url"], "https://example.com/photo.jpg");
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn convert_attachments_bytes_source() {
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            filename: Some("test.png".to_string()),
        }];
        let blocks = convert_attachments_to_images(&attachments);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Image { source } = &blocks[0] {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "base64");
            assert_eq!(json["media_type"], "image/png");
            assert!(!json["data"].as_str().unwrap().is_empty());
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn convert_attachments_empty() {
        let blocks = convert_attachments_to_images(&[]);
        assert!(blocks.is_empty());
    }
}
