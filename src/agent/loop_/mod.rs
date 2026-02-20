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
use crate::auth::AuthBroker;
use crate::config::Config;
use crate::memory::{self, Memory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::{EntityRateLimiter, PermissionStore, SecurityPolicy};
use crate::tools;
use crate::tools::ToolRegistry;
use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;

// ── Test-only crate imports (visible to tests via super::*) ──────
#[cfg(test)]
use crate::memory::MemorySource;
#[cfg(test)]
use crate::observability::NoopObserver;

pub async fn run(
    config: Arc<Config>,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let _runtime = runtime::create_runtime(&config.runtime)?;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let auth_broker = AuthBroker::load_or_init(&config)?;

    let mem = init_memory(&config, &auth_broker)?;
    let registry = init_tools(&config, &security, &mem);
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

    let (answer_provider, reflect_provider) =
        resolve_providers(&config, &auth_broker, provider_name)?;

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    let system_prompt = build_agent_system_prompt(&config, model_name);
    let turn_params = MainSessionTurnParams {
        answer_provider: answer_provider.as_ref(),
        reflect_provider: reflect_provider.as_ref(),
        system_prompt: &system_prompt,
        model_name,
        temperature,
        registry,
        max_tool_iterations: config.autonomy.max_tool_loop_iterations,
        rate_limiter,
        permission_store,
    };

    let (token_sum, saw_token_usage) =
        run_session(&config, &security, &mem, &turn_params, message, &observer).await?;

    let duration = Instant::now().elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        duration,
        tokens_used: saw_token_usage.then_some(token_sum),
    });

    Ok(())
}

fn init_memory(config: &Config, auth_broker: &AuthBroker) -> Result<Arc<dyn Memory>> {
    let memory_api_key = auth_broker.resolve_memory_api_key(&config.memory);
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        memory_api_key.as_deref(),
    )?);
    tracing::info!(backend = mem.name(), "Memory initialized");
    Ok(mem)
}

fn init_tools(
    config: &Config,
    security: &Arc<SecurityPolicy>,
    mem: &Arc<dyn Memory>,
) -> Arc<ToolRegistry> {
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools = tools::all_tools(
        security,
        Arc::clone(mem),
        composio_key,
        &config.browser,
        &config.tools,
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
        )?;
    let reflect_api_key = auth_broker.resolve_provider_api_key(provider_name);
    let reflect_provider: Box<dyn Provider> = providers::create_provider_with_oauth_recovery(
        config,
        provider_name,
        reflect_api_key.as_deref(),
    )?;

    Ok((answer_provider, reflect_provider))
}

fn build_agent_system_prompt(config: &Config, model_name: &str) -> String {
    let skills = crate::skills::load_skills(&config.workspace_dir);
    let tool_descs =
        crate::tools::tool_descriptions(config.browser.enabled, config.composio.enabled);
    let prompt_options = crate::channels::SystemPromptOptions {
        persona_state_mirror_filename: if config.persona.enabled_main_session {
            Some(config.persona.state_mirror_filename.clone())
        } else {
            None
        },
    };
    crate::channels::build_system_prompt_with_options(
        &config.workspace_dir,
        model_name,
        &tool_descs,
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
        )
        .await?;
        if let Some(tokens) = outcome.tokens_used {
            token_sum = token_sum.saturating_add(tokens);
            saw_token_usage = true;
        }
        println!("{}", outcome.response);
    } else {
        println!("🦀 AsteronIris Interactive Mode");
        println!("Type /quit to exit.\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::channels::CliChannel::new();

        let listen_handle = tokio::spawn(async move {
            let _ = crate::channels::Channel::listen(&cli, tx).await;
        });

        while let Some(msg) = rx.recv().await {
            let outcome = execute_main_session_turn_with_metrics(
                config,
                security,
                Arc::clone(mem),
                turn_params,
                &msg.content,
                observer,
            )
            .await?;
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
