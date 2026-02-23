mod context;
mod inference;
mod reflect;
mod session;
mod types;
mod verify_repair;

// ── Public API re-exports ────────────────────────────────────────
#[allow(unused_imports)]
pub use context::build_context_for_integration;
#[allow(unused_imports)]
pub use session::{
    run_main_session_turn_for_integration, run_main_session_turn_for_integration_with_policy,
};
pub use types::IntegrationTurnParams;
pub(super) use types::RuntimeMemoryWriteContext;

// ── Internal re-exports (used by run() and/or tests) ─────────────
use session::execute_main_session_turn_with_metrics;
use types::MainSessionTurnParams;

#[cfg(test)]
use session::execute_main_session_turn;
#[cfg(test)]
use session::execute_main_session_turn_with_accounting;
#[cfg(test)]
use types::PERSONA_PER_TURN_CALL_BUDGET;

// ── Crate imports for run() ──────────────────────────────────────
use crate::config::Config;
use crate::core::memory::{self, Memory};
use crate::core::persona::person_identity::resolve_person_id;
use crate::core::providers::response::ProviderMessage;
use crate::core::providers::{self, Provider};
use crate::core::subagents::{SubagentRuntimeConfig, configure_runtime};
use crate::core::tools;
use crate::core::tools::ToolRegistry;
use crate::runtime;
use crate::runtime::observability::{self, Observer, ObserverEvent};
use crate::security::auth::AuthBroker;
use crate::security::{EntityRateLimiter, PermissionStore, SecurityPolicy};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;

// ── Test-only crate imports (visible to tests via super::*) ──────
#[cfg(test)]
use crate::core::memory::MemorySource;
#[cfg(test)]
use crate::runtime::observability::NoopObserver;

pub async fn run(
    config: Arc<Config>,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let _runtime = runtime::create_runtime(&config.runtime).context("initialize agent runtime")?;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let auth_broker = AuthBroker::load_or_init(&config).context("load auth broker")?;

    let mem = init_memory(&config, &auth_broker).context("initialize memory backend")?;
    let registry = init_tools(&config, &security, &mem, &auth_broker);
    let rate_limiter = Arc::new(EntityRateLimiter::new(
        config.autonomy.max_actions_per_hour,
        config.autonomy.max_actions_per_entity_per_hour,
    ));
    let permission_store = Arc::new(PermissionStore::load(&config.workspace_dir));

    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");
    let model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");
    let system_prompt = build_agent_system_prompt(&config, model_name);

    let (answer_provider, reflect_provider) =
        resolve_providers(&config, &auth_broker, provider_name).context("resolve LLM providers")?;
    let subagent_provider = resolve_subagent_provider(&config, &auth_broker, provider_name)
        .context("resolve sub-agent provider")?;
    configure_runtime(SubagentRuntimeConfig {
        provider: subagent_provider,
        system_prompt: system_prompt.clone(),
        default_model: model_name.to_string(),
        default_temperature: temperature,
    })
    .context("configure sub-agent runtime")?;

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    let person_id = resolve_person_id(&config);
    let turn_params = MainSessionTurnParams {
        answer_provider: answer_provider.as_ref(),
        reflect_provider: reflect_provider.as_ref(),
        person_id: &person_id,
        system_prompt: &system_prompt,
        model_name,
        temperature,
        registry,
        max_tool_iterations: config.autonomy.max_tool_loop_iterations,
        rate_limiter,
        permission_store,
    };

    let (token_sum, saw_token_usage) =
        run_session(&config, &security, &mem, &turn_params, message, &observer)
            .await
            .context("run agent session")?;

    let duration = Instant::now().elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        duration,
        tokens_used: saw_token_usage.then_some(token_sum),
    });

    Ok(())
}

fn init_memory(config: &Config, auth_broker: &AuthBroker) -> Result<Arc<dyn Memory>> {
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(
        memory::create_memory(
            &config.memory,
            &config.workspace_dir,
            memory_api_key.as_deref(),
        )
        .context("create memory backend")?,
    );
    tracing::info!(backend = mem.name(), "Memory initialized");
    Ok(mem)
}

fn init_tools(
    config: &Config,
    security: &Arc<SecurityPolicy>,
    mem: &Arc<dyn Memory>,
    #[cfg_attr(not(feature = "taste"), allow(unused_variables))] auth_broker: &AuthBroker,
) -> Arc<ToolRegistry> {
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
        security,
        Arc::clone(mem),
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
    Arc::new(registry)
}

fn resolve_providers(
    config: &Config,
    auth_broker: &AuthBroker,
    provider_name: &str,
) -> Result<(Box<dyn Provider>, Box<dyn Provider>)> {
    let answer_provider: Box<dyn Provider> =
        providers::create_resilient_provider_with_oauth_recovery(
            config,
            provider_name,
            &config.reliability,
            |name| auth_broker.resolve_provider_api_key(name),
        )
        .context("create resilient answer provider")?;
    let reflect_api_key = auth_broker.resolve_provider_api_key(provider_name);
    let reflect_provider: Box<dyn Provider> = providers::create_provider_with_oauth_recovery(
        config,
        provider_name,
        reflect_api_key.as_deref(),
    )
    .context("create reflect provider")?;

    Ok((answer_provider, reflect_provider))
}

fn resolve_subagent_provider(
    config: &Config,
    auth_broker: &AuthBroker,
    provider_name: &str,
) -> Result<Arc<dyn Provider>> {
    let provider: Box<dyn Provider> = providers::create_resilient_provider_with_oauth_recovery(
        config,
        provider_name,
        &config.reliability,
        |name| auth_broker.resolve_provider_api_key(name),
    )
    .context("create resilient subagent provider")?;
    Ok(Arc::from(provider))
}

fn build_agent_system_prompt(config: &Config, model_name: &str) -> String {
    let skills = crate::plugins::skills::load_skills(&config.workspace_dir);
    let tool_descs = crate::core::tools::tool_descriptions(
        config.browser.enabled,
        config.composio.enabled,
        Some(&config.mcp),
    );
    let prompt_tool_descs: Vec<(&str, &str)> = tool_descs
        .iter()
        .map(|(name, description)| (name.as_str(), description.as_str()))
        .collect();
    let prompt_options = crate::transport::channels::SystemPromptOptions {
        persona_state_mirror_filename: if config.persona.enabled_main_session {
            Some(config.persona.state_mirror_filename.clone())
        } else {
            None
        },
    };
    crate::transport::channels::build_system_prompt_with_options(
        &config.workspace_dir,
        model_name,
        &prompt_tool_descs,
        &skills,
        &prompt_options,
    )
}

async fn run_session(
    config: &Config,
    security: &SecurityPolicy,
    mem: &Arc<dyn Memory>,
    turn_params: &MainSessionTurnParams<'_>,
    message: Option<String>,
    observer: &Arc<dyn Observer>,
) -> Result<(u64, bool)> {
    let mut token_sum = 0_u64;
    let mut saw_token_usage = false;

    if let Some(msg) = message {
        let outcome = execute_main_session_turn_with_metrics(
            config,
            security,
            Arc::clone(mem),
            turn_params,
            &msg,
            observer,
            &[],
        )
        .await
        .context("execute agent session turn")?;
        if let Some(tokens) = outcome.tokens_used {
            token_sum = token_sum.saturating_add(tokens);
            saw_token_usage = true;
        }
        println!("{}", outcome.response);
    } else {
        const MAX_HISTORY_MESSAGES: usize = 20;
        println!("🦀 AsteronIris Interactive Mode");
        println!("Type /quit to exit.\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::transport::channels::CliChannel::new();

        let listen_handle = tokio::spawn(async move {
            let _ = crate::transport::channels::Channel::listen(&cli, tx).await;
        });

        let mut conversation_history: Vec<ProviderMessage> = Vec::new();

        while let Some(msg) = rx.recv().await {
            let outcome = execute_main_session_turn_with_metrics(
                config,
                security,
                Arc::clone(mem),
                turn_params,
                &msg.content,
                observer,
                &conversation_history,
            )
            .await
            .context("execute agent session turn")?;

            // Accumulate conversation history (10-turn sliding window = 20 messages)
            conversation_history.push(ProviderMessage::user(&msg.content));
            conversation_history.push(ProviderMessage {
                role: crate::core::providers::response::MessageRole::Assistant,
                content: vec![crate::core::providers::response::ContentBlock::Text {
                    text: outcome.response.clone(),
                }],
            });

            if conversation_history.len() > MAX_HISTORY_MESSAGES {
                let excess = conversation_history.len() - MAX_HISTORY_MESSAGES;
                conversation_history.drain(..excess);
            }

            if let Some(tokens) = outcome.tokens_used {
                token_sum = token_sum.saturating_add(tokens);
                saw_token_usage = true;
            }
            println!("\n{}\n", outcome.response);
        }

        listen_handle.abort();
    }

    Ok((token_sum, saw_token_usage))
}

#[cfg(test)]
mod tests;
