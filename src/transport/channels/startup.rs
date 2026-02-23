use crate::config::Config;
use crate::core::memory::{self, Memory};
use crate::core::providers::{self, Provider};
use crate::core::tools;
use crate::core::tools::registry::ToolRegistry;
use crate::media::MediaStore;
use crate::security::auth::AuthBroker;
use crate::security::policy::EntityRateLimiter;
use crate::security::{PermissionStore, SecurityPolicy};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::factory;
use super::health::{ChannelHealthState, classify_health_result};
use super::message_handler::handle_channel_message;
use super::policy::ChannelPolicy;
use super::prompt_builder::build_system_prompt;
use super::runtime::{channel_backoff_settings, spawn_supervised_listener};
use super::traits::{Channel, ChannelMessage};

fn build_channel_system_prompt(
    config: &Config,
    workspace: &std::path::Path,
    model: &str,
    skills: &[crate::plugins::skills::Skill],
) -> String {
    let tool_descs = crate::core::tools::tool_descriptions(
        config.browser.enabled,
        config.composio.enabled,
        Some(&config.mcp),
    );
    let prompt_tool_descs: Vec<(&str, &str)> = tool_descs
        .iter()
        .map(|(name, description)| (name.as_str(), description.as_str()))
        .collect();
    build_system_prompt(workspace, model, &prompt_tool_descs, skills)
}

pub(super) struct ChannelRuntime {
    pub(super) config: Arc<Config>,
    pub(super) security: Arc<SecurityPolicy>,
    pub(super) provider: Arc<dyn Provider>,
    pub(super) registry: Arc<ToolRegistry>,
    pub(super) rate_limiter: Arc<EntityRateLimiter>,
    pub(super) permission_store: Arc<PermissionStore>,
    pub(super) model: String,
    pub(super) temperature: f64,
    pub(super) mem: Arc<dyn Memory>,
    pub(super) media_store: Option<Arc<MediaStore>>,
    pub(super) system_prompt: String,
    pub(super) channels: Vec<Arc<dyn Channel>>,
    pub(super) channel_policies: HashMap<String, ChannelPolicy>,
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

#[allow(clippy::too_many_lines)]
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
    #[cfg(feature = "taste")]
    let taste_provider: Option<Arc<dyn crate::core::providers::Provider>> = if config.taste.enabled
    {
        let provider_name = config.default_provider.as_deref().unwrap_or("anthropic");
        let api_key = auth_broker.resolve_provider_api_key(provider_name);
        providers::create_provider_with_oauth_recovery(config, provider_name, api_key.as_deref())
            .ok()
            .map(|p| Arc::from(p) as Arc<dyn crate::core::providers::Provider>)
    } else {
        None
    };
    #[cfg(not(feature = "taste"))]
    let taste_provider: Option<Arc<dyn crate::core::providers::Provider>> = None;
    let tools = tools::all_tools(
        &security,
        Arc::clone(&mem),
        composio_key,
        &config.browser,
        &config.tools,
        Some(&config.mcp),
        &config.taste,
        taste_provider,
        config
            .default_model
            .as_deref()
            .unwrap_or("anthropic/claude-sonnet-4-20250514"),
    );
    let middleware = tools::default_middleware_chain();
    let mut registry = ToolRegistry::new(middleware);
    for tool in tools {
        registry.register(tool);
    }

    let workspace = config.workspace_dir.clone();
    let skills = crate::plugins::skills::load_skills(&workspace);
    let system_prompt = build_channel_system_prompt(config, &workspace, &model, &skills);

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

    crate::runtime::diagnostics::health::mark_component_ok("channels");

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
    use crate::core::providers::response::ContentBlock;
    use crate::core::tools::OutputAttachment;
    use crate::media::MediaProcessor;
    use crate::transport::channels::attachments::{
        attachment_to_image_block, convert_attachments_to_images, encode_base64,
        fallback_attachment_description, load_attachment_bytes,
        output_attachment_to_media_attachment, prepare_channel_input_and_images,
    };
    use crate::transport::channels::traits::{MediaAttachment, MediaData};
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
        let processor = MediaProcessor::new();
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

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, None, &processor).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_adds_description_and_stores() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Bytes(vec![1, 2, 3, 4]),
            filename: Some("sound.mp3".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert!(input.starts_with("[Audio: sound.mp3 (audio/mpeg, 4 bytes"));
        assert!(input.ends_with("\n\nhello"));
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_image_bytes_remains_inline_and_is_stored() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            filename: Some("img.png".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_image_url_is_downloaded_stored_and_forwarded_as_url() {
        let processor = MediaProcessor::new();
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
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

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
        let processor = MediaProcessor::new();
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
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert!(input.contains("[Audio: voice.mp3 (audio/mpeg, 4 bytes"));
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_url_download_failure_falls_back() {
        let processor = MediaProcessor::new();
        let attachments = vec![MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Url("http://127.0.0.1:9/missing.pdf".to_string()),
            filename: Some("missing.pdf".to_string()),
        }];

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(
            input,
            "[Attachment: missing.pdf (application/pdf)]\n\nhello"
        );
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 0);
    }

    #[tokio::test]
    async fn prepare_channel_input_store_failure_falls_back_for_non_image() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 0);
        let attachments = vec![MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Bytes(vec![1]),
            filename: Some("doc.pdf".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(
            input,
            "[Attachment: doc.pdf (application/pdf, 1KB)]\n\nhello"
        );
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 0);
    }

    #[tokio::test]
    async fn prepare_channel_input_mixed_attachments_preserves_images_and_adds_text_prefix() {
        let processor = MediaProcessor::new();
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
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert!(input.starts_with("[Audio: clip.mp3 (audio/mpeg, 3 bytes"));
        assert!(input.ends_with("\n\nhello"));
        assert_eq!(images.len(), 1);
        assert_eq!(stored_file_count(&temp_dir), 2);
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_reads_bytes_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("generated.bin");
        fs::write(&path, [1_u8, 2, 3, 4]).unwrap();
        let attachment = OutputAttachment::from_path(
            "application/octet-stream",
            path.to_string_lossy().to_string(),
            Some("generated.bin".to_string()),
        );

        let media = output_attachment_to_media_attachment(&attachment)
            .await
            .unwrap();
        match media.data {
            MediaData::Bytes(bytes) => assert_eq!(bytes, vec![1, 2, 3, 4]),
            MediaData::Url(_) => panic!("expected bytes media data"),
        }
        assert_eq!(media.mime_type, "application/octet-stream");
        assert_eq!(media.filename.as_deref(), Some("generated.bin"));
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_maps_url_variant() {
        let attachment = OutputAttachment::from_url(
            "image/png",
            "https://example.com/a.png",
            Some("a.png".to_string()),
        );

        let media = output_attachment_to_media_attachment(&attachment)
            .await
            .unwrap();
        match media.data {
            MediaData::Url(url) => assert_eq!(url, "https://example.com/a.png"),
            MediaData::Bytes(_) => panic!("expected url media data"),
        }
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_missing_path_returns_none() {
        let attachment = OutputAttachment::from_path(
            "image/png",
            "/tmp/does-not-exist.png",
            Some("missing.png".to_string()),
        );

        let media = output_attachment_to_media_attachment(&attachment).await;
        assert!(media.is_none());
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_without_location_returns_none() {
        let attachment = OutputAttachment {
            mime_type: "image/png".to_string(),
            filename: Some("img.png".to_string()),
            path: None,
            url: None,
        };

        let media = output_attachment_to_media_attachment(&attachment).await;
        assert!(media.is_none());
    }
}
