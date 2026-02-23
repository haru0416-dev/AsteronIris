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

use super::super::factory;
use super::super::policy::ChannelPolicy;
use super::super::traits::Channel;
use super::prompt::build_channel_system_prompt;

pub(in super::super) struct ChannelRuntime {
    pub(in super::super) config: Arc<Config>,
    pub(in super::super) security: Arc<SecurityPolicy>,
    pub(in super::super) provider: Arc<dyn Provider>,
    pub(in super::super) registry: Arc<ToolRegistry>,
    pub(in super::super) rate_limiter: Arc<EntityRateLimiter>,
    pub(in super::super) permission_store: Arc<PermissionStore>,
    pub(in super::super) model: String,
    pub(in super::super) temperature: f64,
    pub(in super::super) mem: Arc<dyn Memory>,
    pub(in super::super) media_store: Option<Arc<MediaStore>>,
    pub(in super::super) system_prompt: String,
    pub(in super::super) channels: Vec<Arc<dyn Channel>>,
    pub(in super::super) channel_policies: HashMap<String, ChannelPolicy>,
}

#[allow(clippy::too_many_lines)]
pub(super) async fn init_channel_runtime(config: &Arc<Config>) -> Result<ChannelRuntime> {
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
