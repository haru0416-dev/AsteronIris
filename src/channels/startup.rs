use crate::agent::tool_loop::{LoopStopReason, ToolLoop};
use crate::auth::AuthBroker;
use crate::config::Config;
use crate::media::{MediaProcessor, MediaStore};
use crate::memory::{self, Memory};
use crate::providers::response::ContentBlock;
use crate::providers::{self, Provider};
use crate::security::policy::EntityRateLimiter;
use crate::security::{PermissionStore, SecurityPolicy, broker_for_channel};
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
use super::traits::{Channel, ChannelMessage, MediaAttachment, MediaData};

fn convert_attachments_to_images(attachments: &[MediaAttachment]) -> Vec<ContentBlock> {
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
    // `data_encoding`/`base64` are not direct dependencies in this crate, so this
    // local encoder is kept until one of those crates is available here.
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

fn attachment_to_image_block(attachment: &MediaAttachment) -> Option<ContentBlock> {
    use crate::providers::response::ImageSource;

    if !attachment.mime_type.starts_with("image/") {
        return None;
    }

    let source = match &attachment.data {
        MediaData::Url(url) => ImageSource::url(url),
        MediaData::Bytes(bytes) => ImageSource::base64(&attachment.mime_type, encode_base64(bytes)),
    };
    Some(ContentBlock::Image { source })
}

async fn load_attachment_bytes(attachment: &MediaAttachment) -> Result<Vec<u8>> {
    match &attachment.data {
        MediaData::Bytes(bytes) => Ok(bytes.clone()),
        MediaData::Url(url) => {
            let response = reqwest::get(url).await?;
            let response = response.error_for_status()?;
            let bytes = response.bytes().await?;
            Ok(bytes.to_vec())
        }
    }
}

fn fallback_attachment_description(
    attachment: &MediaAttachment,
    size_bytes: Option<usize>,
) -> String {
    let filename = attachment.filename.as_deref().unwrap_or("unnamed");
    let size_part = size_bytes
        .map(|bytes| format!(", {}KB", bytes.div_ceil(1024)))
        .unwrap_or_default();
    format!(
        "[Attachment: {filename} ({}{size_part})]",
        attachment.mime_type
    )
}

async fn prepare_channel_input_and_images(
    model_input: &str,
    attachments: &[MediaAttachment],
    media_store: Option<&Arc<MediaStore>>,
) -> (String, Vec<ContentBlock>) {
    let Some(store) = media_store else {
        return (
            model_input.to_string(),
            convert_attachments_to_images(attachments),
        );
    };
    let processor = MediaProcessor::new();
    let mut attachment_descriptions = Vec::new();
    let mut image_blocks = Vec::new();

    for attachment in attachments {
        match load_attachment_bytes(attachment).await {
            Ok(bytes) => match store.store(&bytes, attachment.filename.as_deref()) {
                Ok(stored) => {
                    if !attachment.mime_type.starts_with("image/") {
                        match processor.describe(&stored, &bytes).await {
                            Ok(description) => attachment_descriptions.push(description),
                            Err(error) => {
                                tracing::warn!(
                                    channel_attachment = ?attachment.filename,
                                    error = %error,
                                    "failed to describe non-image attachment"
                                );
                                attachment_descriptions.push(fallback_attachment_description(
                                    attachment,
                                    Some(bytes.len()),
                                ));
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        channel_attachment = ?attachment.filename,
                        error = %error,
                        "failed to persist attachment"
                    );
                    if !attachment.mime_type.starts_with("image/") {
                        attachment_descriptions.push(fallback_attachment_description(
                            attachment,
                            Some(bytes.len()),
                        ));
                    }
                }
            },
            Err(error) => {
                tracing::warn!(
                    channel_attachment = ?attachment.filename,
                    error = %error,
                    "failed to load attachment bytes"
                );
                if !attachment.mime_type.starts_with("image/") {
                    attachment_descriptions.push(fallback_attachment_description(attachment, None));
                }
            }
        }

        if let Some(block) = attachment_to_image_block(attachment) {
            image_blocks.push(block);
        }
    }

    if attachment_descriptions.is_empty() {
        (model_input.to_string(), image_blocks)
    } else {
        let prefix = attachment_descriptions.join("\n");
        (format!("{prefix}\n\n{model_input}"), image_blocks)
    }
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
    media_store: Option<Arc<MediaStore>>,
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
    let media_store = if config.media.enabled {
        let workspace_dir = config.workspace_dir.to_string_lossy().into_owned();
        Some(Arc::new(MediaStore::new(&config.media, &workspace_dir)?))
    } else {
        None
    };
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
        media_store,
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

    let global_autonomy = rt.config.autonomy.effective_autonomy_level();
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
        approval_broker: Some(broker_for_channel(&msg.channel)),
    };
    let tool_loop = ToolLoop::new(
        Arc::clone(&rt.registry),
        rt.config.autonomy.max_tool_loop_iterations,
    );
    let (message_input, image_blocks) = prepare_channel_input_and_images(
        &ingress.model_input,
        &msg.attachments,
        rt.media_store.as_ref(),
    )
    .await;

    match tool_loop
        .run(
            rt.provider.as_ref(),
            &rt.system_prompt,
            &message_input,
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
    use std::fs;

    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_media_store(temp_dir: &TempDir, max_file_size_mb: u64) -> Arc<MediaStore> {
        let workspace = temp_dir.path().to_string_lossy().into_owned();
        let config = crate::media::types::MediaConfig {
            enabled: true,
            storage_dir: None,
            max_file_size_mb,
        };
        Arc::new(MediaStore::new(&config, &workspace).unwrap())
    }

    fn stored_file_count(temp_dir: &TempDir) -> usize {
        fs::read_dir(temp_dir.path().join("media"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy() != "media.db")
            .count()
    }

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

    #[test]
    fn attachment_to_image_block_returns_none_for_non_images() {
        let attachment = MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Bytes(vec![1, 2, 3]),
            filename: Some("clip.mp3".to_string()),
        };

        assert!(attachment_to_image_block(&attachment).is_none());
    }

    #[test]
    fn attachment_to_image_block_returns_url_variant() {
        let attachment = MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Url("https://example.com/a.png".to_string()),
            filename: Some("a.png".to_string()),
        };

        let block = attachment_to_image_block(&attachment).unwrap();
        if let ContentBlock::Image { source } = block {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "url");
            assert_eq!(json["url"], "https://example.com/a.png");
        } else {
            panic!("expected image block");
        }
    }

    #[test]
    fn fallback_attachment_description_includes_size_when_known() {
        let attachment = MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Bytes(vec![0_u8; 2048]),
            filename: Some("doc.pdf".to_string()),
        };

        let description = fallback_attachment_description(&attachment, Some(2048));
        assert_eq!(description, "[Attachment: doc.pdf (application/pdf, 2KB)]");
    }

    #[test]
    fn fallback_attachment_description_omits_size_when_unknown() {
        let attachment = MediaAttachment {
            mime_type: "application/octet-stream".to_string(),
            data: MediaData::Url("https://example.com/blob".to_string()),
            filename: None,
        };

        let description = fallback_attachment_description(&attachment, None);
        assert_eq!(
            description,
            "[Attachment: unnamed (application/octet-stream)]"
        );
    }

    #[tokio::test]
    async fn load_attachment_bytes_returns_raw_bytes_variant() {
        let attachment = MediaAttachment {
            mime_type: "text/plain".to_string(),
            data: MediaData::Bytes(vec![7, 8, 9]),
            filename: Some("note.txt".to_string()),
        };

        let loaded = load_attachment_bytes(&attachment).await.unwrap();
        assert_eq!(loaded, vec![7, 8, 9]);
    }

    #[tokio::test]
    async fn load_attachment_bytes_downloads_url_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 3, 3, 7]))
            .mount(&server)
            .await;

        let attachment = MediaAttachment {
            mime_type: "application/octet-stream".to_string(),
            data: MediaData::Url(format!("{}/file.bin", server.uri())),
            filename: Some("file.bin".to_string()),
        };

        let loaded = load_attachment_bytes(&attachment).await.unwrap();
        assert_eq!(loaded, vec![1, 3, 3, 7]);
    }

    #[tokio::test]
    async fn prepare_channel_input_media_disabled_keeps_behavior() {
        let attachments = vec![
            MediaAttachment {
                mime_type: "image/png".to_string(),
                data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
                filename: Some("inline.png".to_string()),
            },
            MediaAttachment {
                mime_type: "audio/mpeg".to_string(),
                data: MediaData::Bytes(vec![1, 2, 3]),
                filename: Some("sound.mp3".to_string()),
            },
        ];

        let (input, images) = prepare_channel_input_and_images("hello", &attachments, None).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_adds_description_and_stores() {
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Bytes(vec![1, 2, 3, 4]),
            filename: Some("sound.mp3".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert!(input.starts_with("[Audio: sound.mp3 (audio/mpeg, 4 bytes)"));
        assert!(input.ends_with("\n\nhello"));
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_image_bytes_remains_inline_and_is_stored() {
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            filename: Some("img.png".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_image_url_is_downloaded_stored_and_forwarded_as_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/img.png"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            )
            .mount(&server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Url(format!("{}/img.png", server.uri())),
            filename: Some("img.png".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
        if let ContentBlock::Image { source } = &images[0] {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "url");
        } else {
            panic!("expected image block");
        }
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_url_downloads_and_describes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/voice.mp3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/mpeg")
                    .set_body_bytes(vec![0x49, 0x44, 0x33, 0x00]),
            )
            .mount(&server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Url(format!("{}/voice.mp3", server.uri())),
            filename: Some("voice.mp3".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert!(input.contains("[Audio: voice.mp3 (audio/mpeg, 4 bytes)"));
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_url_download_failure_falls_back() {
        let attachments = vec![MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Url("http://127.0.0.1:9/missing.pdf".to_string()),
            filename: Some("missing.pdf".to_string()),
        }];

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert_eq!(
            input,
            "[Attachment: missing.pdf (application/pdf)]\n\nhello"
        );
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 0);
    }

    #[tokio::test]
    async fn prepare_channel_input_store_failure_falls_back_for_non_image() {
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 0);
        let attachments = vec![MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Bytes(vec![1]),
            filename: Some("doc.pdf".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert_eq!(
            input,
            "[Attachment: doc.pdf (application/pdf, 1KB)]\n\nhello"
        );
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 0);
    }

    #[tokio::test]
    async fn prepare_channel_input_mixed_attachments_preserves_images_and_adds_text_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![
            MediaAttachment {
                mime_type: "audio/mpeg".to_string(),
                data: MediaData::Bytes(vec![1, 2, 3]),
                filename: Some("clip.mp3".to_string()),
            },
            MediaAttachment {
                mime_type: "image/png".to_string(),
                data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
                filename: Some("img.png".to_string()),
            },
        ];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store)).await;

        assert!(input.starts_with("[Audio: clip.mp3 (audio/mpeg, 3 bytes)"));
        assert!(input.ends_with("\n\nhello"));
        assert_eq!(images.len(), 1);
        assert_eq!(stored_file_count(&temp_dir), 2);
    }
}
