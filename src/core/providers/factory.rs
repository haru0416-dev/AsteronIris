use super::compatible::{AuthStyle, OpenAiCompatibleProvider};
use super::oauth_recovery::OAuthRecoveryProvider;
use super::reliable::ReliableProvider;
use super::traits::Provider;
use std::sync::Arc;

/// Resolve API key for a provider from config and environment variables.
///
/// Resolution order:
/// 1. Explicitly provided `api_key` parameter (trimmed, filtered if empty)
/// 2. Provider-specific environment variable (e.g., `ANTHROPIC_OAUTH_TOKEN`, `OPENROUTER_API_KEY`)
/// 3. Generic fallback variables (`ASTERONIRIS_API_KEY`, `API_KEY`)
///
/// For Anthropic, the provider-specific env var is `ANTHROPIC_OAUTH_TOKEN` (for setup-tokens)
/// followed by `ANTHROPIC_API_KEY` (for regular API keys).
fn resolve_api_key(name: &str, explicit_api_key: Option<&str>) -> Option<String> {
    if let Some(key) = explicit_api_key.map(str::trim).filter(|k| !k.is_empty()) {
        return Some(key.to_string());
    }

    let provider_env_candidates: Vec<&str> = match name {
        "anthropic" => vec!["ANTHROPIC_OAUTH_TOKEN", "ANTHROPIC_API_KEY"],
        "openrouter" => vec!["OPENROUTER_API_KEY"],
        "openai" => vec!["OPENAI_API_KEY"],
        "openai-codex" => vec!["OPENAI_CODEX_API_KEY", "OPENAI_API_KEY"],
        "venice" => vec!["VENICE_API_KEY"],
        "groq" => vec!["GROQ_API_KEY"],
        "mistral" => vec!["MISTRAL_API_KEY"],
        "deepseek" => vec!["DEEPSEEK_API_KEY"],
        "xai" | "grok" => vec!["XAI_API_KEY"],
        "together" | "together-ai" => vec!["TOGETHER_API_KEY"],
        "fireworks" | "fireworks-ai" => vec!["FIREWORKS_API_KEY"],
        "perplexity" => vec!["PERPLEXITY_API_KEY"],
        "cohere" => vec!["COHERE_API_KEY"],
        "moonshot" | "kimi" => vec!["MOONSHOT_API_KEY"],
        "glm" | "zhipu" => vec!["GLM_API_KEY"],
        "minimax" => vec!["MINIMAX_API_KEY"],
        "qianfan" | "baidu" => vec!["QIANFAN_API_KEY"],
        "zai" | "z.ai" => vec!["ZAI_API_KEY"],
        "synthetic" => vec!["SYNTHETIC_API_KEY"],
        "opencode" | "opencode-zen" => vec!["OPENCODE_API_KEY"],
        "vercel" | "vercel-ai" => vec!["VERCEL_API_KEY"],
        "cloudflare" | "cloudflare-ai" => vec!["CLOUDFLARE_API_KEY"],
        _ => vec![],
    };

    for env_var in provider_env_candidates {
        if let Ok(value) = std::env::var(env_var) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    for env_var in ["ASTERONIRIS_API_KEY", "API_KEY"] {
        if let Ok(value) = std::env::var(env_var) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn compatible_provider_spec(name: &str) -> Option<(&'static str, &'static str)> {
    let spec = match name {
        "venice" => ("Venice", "https://api.venice.ai"),
        "vercel" | "vercel-ai" => ("Vercel AI Gateway", "https://api.vercel.ai"),
        "cloudflare" | "cloudflare-ai" => (
            "Cloudflare AI Gateway",
            "https://gateway.ai.cloudflare.com/v1",
        ),
        "moonshot" | "kimi" => ("Moonshot", "https://api.moonshot.cn"),
        "synthetic" => ("Synthetic", "https://api.synthetic.com"),
        "opencode" | "opencode-zen" => ("OpenCode Zen", "https://api.opencode.ai"),
        "zai" | "z.ai" => ("Z.AI", "https://api.z.ai/api/coding/paas/v4"),
        "glm" | "zhipu" => ("GLM", "https://open.bigmodel.cn/api/paas"),
        "minimax" => ("MiniMax", "https://api.minimax.chat"),
        "bedrock" | "aws-bedrock" => (
            "Amazon Bedrock",
            "https://bedrock-runtime.us-east-1.amazonaws.com",
        ),
        "qianfan" | "baidu" => ("Qianfan", "https://aip.baidubce.com"),
        "groq" => ("Groq", "https://api.groq.com/openai"),
        "mistral" => ("Mistral", "https://api.mistral.ai"),
        "xai" | "grok" => ("xAI", "https://api.x.ai"),
        "deepseek" => ("DeepSeek", "https://api.deepseek.com"),
        "together" | "together-ai" => ("Together AI", "https://api.together.xyz"),
        "fireworks" | "fireworks-ai" => ("Fireworks AI", "https://api.fireworks.ai/inference"),
        "perplexity" => ("Perplexity", "https://api.perplexity.ai"),
        "cohere" => ("Cohere", "https://api.cohere.com/compatibility"),
        "copilot" | "github-copilot" => ("GitHub Copilot", "https://api.githubcopilot.com"),
        "openai-codex" => (
            "OpenAI Codex",
            "https://chatgpt.com/backend-api/codex/responses",
        ),
        _ => return None,
    };
    Some(spec)
}

fn create_custom_provider(
    name: &str,
    api_key: Option<&str>,
) -> Option<anyhow::Result<Box<dyn Provider>>> {
    if let Some(base_url) = name.strip_prefix("custom:") {
        Some(if base_url.is_empty() {
            Err(anyhow::anyhow!(
                "Custom provider requires a URL. Format: custom:https://your-api.com"
            ))
        } else {
            Ok(Box::new(OpenAiCompatibleProvider::new(
                "Custom",
                base_url,
                api_key,
                AuthStyle::Bearer,
            )))
        })
    } else if let Some(base_url) = name.strip_prefix("anthropic-custom:") {
        Some(if base_url.is_empty() {
            Err(anyhow::anyhow!(
                "Anthropic-custom provider requires a URL. Format: anthropic-custom:https://your-api.com"
            ))
        } else {
            Ok(Box::new(
                super::anthropic::AnthropicProvider::with_base_url(api_key, Some(base_url)),
            ))
        })
    } else {
        None
    }
}

pub fn create_provider(name: &str, api_key: Option<&str>) -> anyhow::Result<Box<dyn Provider>> {
    let resolved_key = resolve_api_key(name, api_key);
    let api_key = resolved_key.as_deref();

    // ── Primary providers (custom implementations) ───────
    match name {
        "openrouter" => {
            return Ok(Box::new(super::openrouter::OpenRouterProvider::new(
                api_key,
            )));
        }
        "anthropic" => return Ok(Box::new(super::anthropic::AnthropicProvider::new(api_key))),
        "openai" => return Ok(Box::new(super::openai::OpenAiProvider::new(api_key))),
        "ollama" => return Ok(Box::new(super::ollama::OllamaProvider::new(None))),
        "gemini" | "google" | "google-gemini" => {
            return Ok(Box::new(super::gemini::GeminiProvider::new(api_key)));
        }
        _ => {}
    }

    // ── OpenAI-compatible providers ──────────────────────
    if let Some((display_name, base_url)) = compatible_provider_spec(name) {
        return Ok(Box::new(OpenAiCompatibleProvider::new(
            display_name,
            base_url,
            api_key,
            AuthStyle::Bearer,
        )));
    }

    if let Some(result) = create_custom_provider(name, api_key) {
        return result;
    }

    anyhow::bail!(
        "Unknown provider: {name}. Check README for supported providers or run `asteroniris onboard --interactive` to reconfigure.\n\
         Tip: Use \"custom:https://your-api.com\" for OpenAI-compatible endpoints.\n\
         Tip: Use \"anthropic-custom:https://your-api.com\" for Anthropic-compatible endpoints."
    )
}

fn create_provider_with_runtime_recovery(
    config: &crate::config::Config,
    name: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Provider>> {
    let provider_name = name.to_string();
    let initial_provider: Arc<dyn Provider> = Arc::from(create_provider(name, api_key)?);
    let config = Arc::new(config.clone());

    let recover = {
        let config = Arc::clone(&config);
        Arc::new(move |provider: &str| {
            crate::security::auth::recover_oauth_profile_for_provider(&config, provider)
        })
    };

    let rebuild = {
        let config = Arc::clone(&config);
        Arc::new(move |provider: &str| {
            let broker = crate::security::auth::AuthBroker::load_or_init(&config)?;
            let refreshed_key = broker.resolve_provider_api_key(provider);
            Ok(
                Arc::from(create_provider(provider, refreshed_key.as_deref())?)
                    as Arc<dyn Provider>,
            )
        })
    };

    Ok(Box::new(OAuthRecoveryProvider::new(
        &provider_name,
        initial_provider,
        recover,
        rebuild,
    )))
}

pub fn create_provider_with_oauth_recovery(
    config: &crate::config::Config,
    name: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Box<dyn Provider>> {
    create_provider_with_runtime_recovery(config, name, api_key)
}

pub fn create_resilient_provider_with_resolver<F>(
    primary_name: &str,
    reliability: &crate::config::ReliabilityConfig,
    mut resolve_api_key_for_provider: F,
) -> anyhow::Result<Box<dyn Provider>>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut providers: Vec<(String, Box<dyn Provider>)> =
        Vec::with_capacity(1 + reliability.fallback_providers.len());

    let primary_key = resolve_api_key_for_provider(primary_name);
    providers.push((
        primary_name.to_string(),
        create_provider(primary_name, primary_key.as_deref())?,
    ));

    for fallback in &reliability.fallback_providers {
        if fallback == primary_name || providers.iter().any(|(name, _)| name == fallback) {
            continue;
        }

        let fallback_key = resolve_api_key_for_provider(fallback);

        match create_provider(fallback, fallback_key.as_deref()) {
            Ok(provider) => providers.push((fallback.clone(), provider)),
            Err(e) => {
                tracing::warn!(
                    fallback_provider = fallback,
                    "Ignoring invalid fallback provider: {e}"
                );
            }
        }
    }

    Ok(Box::new(ReliableProvider::new(
        providers,
        reliability.provider_retries,
        reliability.provider_backoff_ms,
    )))
}

pub fn create_resilient_provider_with_oauth_recovery<F>(
    config: &crate::config::Config,
    primary_name: &str,
    reliability: &crate::config::ReliabilityConfig,
    mut resolve_api_key_for_provider: F,
) -> anyhow::Result<Box<dyn Provider>>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut providers: Vec<(String, Box<dyn Provider>)> = Vec::new();

    let primary_key = resolve_api_key_for_provider(primary_name);
    providers.push((
        primary_name.to_string(),
        create_provider_with_runtime_recovery(config, primary_name, primary_key.as_deref())?,
    ));

    for fallback in &reliability.fallback_providers {
        if fallback == primary_name || providers.iter().any(|(name, _)| name == fallback) {
            continue;
        }

        let fallback_key = resolve_api_key_for_provider(fallback);

        match create_provider_with_runtime_recovery(config, fallback, fallback_key.as_deref()) {
            Ok(provider) => providers.push((fallback.clone(), provider)),
            Err(e) => {
                tracing::warn!(
                    fallback_provider = fallback,
                    "Ignoring invalid fallback provider: {e}"
                );
            }
        }
    }

    Ok(Box::new(ReliableProvider::new(
        providers,
        reliability.provider_retries,
        reliability.provider_backoff_ms,
    )))
}

pub fn create_resilient_provider(
    primary_name: &str,
    api_key: Option<&str>,
    reliability: &crate::config::ReliabilityConfig,
) -> anyhow::Result<Box<dyn Provider>> {
    create_resilient_provider_with_resolver(primary_name, reliability, |provider_name| {
        resolve_api_key(provider_name, api_key)
    })
}
