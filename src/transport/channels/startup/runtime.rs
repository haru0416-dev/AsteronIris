use crate::config::Config;
use crate::llm::traits::Provider;
use crate::memory::traits::Memory;
use crate::security::policy::{EntityRateLimiter, SecurityPolicy};
use crate::tools::middleware::default_middleware_chain;
use crate::tools::registry::ToolRegistry;
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
    pub(in super::super) model: String,
    pub(in super::super) temperature: f64,
    pub(in super::super) mem: Arc<dyn Memory>,
    pub(in super::super) system_prompt: String,
    pub(in super::super) channels: Vec<Arc<dyn Channel>>,
    pub(in super::super) channel_policies: HashMap<String, ChannelPolicy>,
}

#[allow(clippy::too_many_lines)]
pub(super) async fn init_channel_runtime(config: &Arc<Config>) -> Result<ChannelRuntime> {
    let config_api_key = config.api_key.clone();
    let provider: Arc<dyn Provider> = Arc::from(
        crate::llm::factory::create_resilient_provider_with_oauth_recovery(
            config,
            config.default_provider.as_deref().unwrap_or("openrouter"),
            &config.reliability,
            move |name| crate::llm::factory::resolve_api_key(name, config_api_key.as_deref()),
        )?,
    );

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

    let mem: Arc<dyn Memory> = Arc::from(
        crate::memory::factory::create_memory(
            &config.memory,
            &config.workspace_dir,
            config.api_key.as_deref(),
        )
        .await?,
    );

    let tools = crate::tools::all_tools(Arc::clone(&mem));
    let mut registry = ToolRegistry::new(default_middleware_chain());
    for tool in tools {
        registry.register(tool);
    }

    let workspace = config.workspace_dir.clone();
    let system_prompt = build_channel_system_prompt(config, &workspace, &model);

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
        model,
        temperature,
        mem,
        system_prompt,
        channels,
        channel_policies,
    })
}
