use anyhow::Result;
use dialoguer::{Input, Select};

use crate::ui::style as ui;

use super::super::domain::{provider_env_var, validate_base_url};
use super::super::view::print_bullet;

#[allow(clippy::too_many_lines)]
pub fn setup_provider() -> Result<(String, String, String)> {
    // ── Tier selection ──
    let tiers = vec![
        format!("› {}", t!("onboard.provider.tier_recommended")),
        format!("› {}", t!("onboard.provider.tier_fast")),
        format!("› {}", t!("onboard.provider.tier_gateway")),
        format!("› {}", t!("onboard.provider.tier_specialized")),
        format!("› {}", t!("onboard.provider.tier_local")),
        format!("› {}", t!("onboard.provider.tier_custom")),
    ];

    let tier_idx = Select::new()
        .with_prompt(format!("  {}", t!("onboard.provider.select_category")))
        .items(&tiers)
        .default(0)
        .interact()?;

    let providers: Vec<(&str, &str)> = match tier_idx {
        0 => vec![
            (
                "openrouter",
                "OpenRouter — 200+ models, 1 API key (recommended)",
            ),
            ("venice", "Venice AI — privacy-first (Llama, Opus)"),
            ("anthropic", "Anthropic — Claude Sonnet & Opus (direct)"),
            ("openai", "OpenAI — GPT-4o, o1, GPT-5 (direct)"),
            ("deepseek", "DeepSeek — V3 & R1 (affordable)"),
            ("mistral", "Mistral — Large & Codestral"),
            ("xai", "xAI — Grok 3 & 4"),
            ("perplexity", "Perplexity — search-augmented AI"),
            (
                "gemini",
                "Google Gemini — Gemini 2.0 Flash & Pro (supports CLI auth)",
            ),
        ],
        1 => vec![
            ("groq", "Groq — ultra-fast LPU inference"),
            ("fireworks", "Fireworks AI — fast open-source inference"),
            ("together", "Together AI — open-source model hosting"),
        ],
        2 => vec![
            ("vercel", "Vercel AI Gateway"),
            ("cloudflare", "Cloudflare AI Gateway"),
            ("bedrock", "Amazon Bedrock — AWS managed models"),
        ],
        3 => vec![
            ("moonshot", "Moonshot — Kimi & Kimi Coding"),
            ("glm", "GLM — ChatGLM / Zhipu models"),
            ("minimax", "MiniMax — MiniMax AI models"),
            ("qianfan", "Qianfan — Baidu AI models"),
            ("zai", "Z.AI — Z.AI inference"),
            ("synthetic", "Synthetic — Synthetic AI models"),
            ("opencode", "OpenCode Zen — code-focused AI"),
            ("cohere", "Cohere — Command R+ & embeddings"),
        ],
        4 => vec![("ollama", "Ollama — local models (Llama, Mistral, Phi)")],
        _ => vec![], // Custom — handled below
    };

    // ── Custom / BYOP flow ──
    if providers.is_empty() {
        println!();
        println!(
            "  {} {}",
            ui::header(t!("onboard.provider.custom_title")),
            ui::dim(format!("— {}", t!("onboard.provider.custom_subtitle")))
        );
        print_bullet(&t!("onboard.provider.custom_desc"));
        print_bullet(&t!("onboard.provider.custom_examples"));
        println!();

        let base_url: String = Input::new()
            .with_prompt(format!("  {}", t!("onboard.provider.base_url_prompt")))
            .interact_text()?;

        let base_url = validate_base_url(&base_url)?;

        let api_key: String = Input::new()
            .with_prompt(format!("  {}", t!("onboard.provider.api_key_prompt")))
            .allow_empty(true)
            .interact_text()?;

        let model: String = Input::new()
            .with_prompt(format!("  {}", t!("onboard.provider.model_prompt")))
            .default("default".into())
            .interact_text()?;

        let provider_name = format!("custom:{base_url}");

        println!(
            "  {} {}",
            ui::success("✓"),
            t!(
                "onboard.provider.confirm",
                provider = ui::value(&provider_name),
                model = ui::value(&model)
            )
        );

        return Ok((provider_name, api_key, model));
    }

    let provider_labels: Vec<&str> = providers.iter().map(|(_, label)| *label).collect();

    let provider_idx = Select::new()
        .with_prompt(format!("  {}", t!("onboard.provider.select_provider")))
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let provider_name = providers[provider_idx].0;

    // ── API key ──
    let api_key = if provider_name == "ollama" {
        print_bullet(&t!("onboard.provider.ollama_no_key"));
        String::new()
    } else if provider_name == "gemini"
        || provider_name == "google"
        || provider_name == "google-gemini"
    {
        if crate::core::providers::gemini::GeminiProvider::has_cli_credentials() {
            print_bullet(&format!(
                "{} {}",
                ui::success("✓"),
                t!("onboard.provider.gemini_cli_detected")
            ));
            print_bullet(&t!("onboard.provider.gemini_cli_reuse"));
            println!();

            let use_cli: bool = dialoguer::Confirm::new()
                .with_prompt(format!("  {}", t!("onboard.provider.gemini_use_cli")))
                .default(true)
                .interact()?;

            if use_cli {
                println!(
                    "  {} {}",
                    ui::success("✓"),
                    t!("onboard.provider.gemini_using_cli")
                );
                String::new()
            } else {
                print_bullet(&t!("onboard.provider.gemini_api_key_url"));
                Input::new()
                    .with_prompt(format!(
                        "  {}",
                        t!("onboard.provider.gemini_api_key_prompt")
                    ))
                    .allow_empty(true)
                    .interact_text()?
            }
        } else if std::env::var("GEMINI_API_KEY").is_ok() {
            print_bullet(&format!(
                "{} {}",
                ui::success("✓"),
                t!("onboard.provider.gemini_env_detected")
            ));
            String::new()
        } else {
            print_bullet(&t!("onboard.provider.gemini_api_key_url"));
            print_bullet(&t!("onboard.provider.gemini_cli_hint"));
            println!();

            Input::new()
                .with_prompt(format!(
                    "  {}",
                    t!("onboard.provider.gemini_api_key_skip_prompt")
                ))
                .allow_empty(true)
                .interact_text()?
        }
    } else {
        let key_url = match provider_name {
            "openrouter" => "https://openrouter.ai/keys",
            "anthropic" => "https://console.anthropic.com/settings/keys",
            "openai" => "https://platform.openai.com/api-keys",
            "venice" => "https://venice.ai/settings/api",
            "groq" => "https://console.groq.com/keys",
            "mistral" => "https://console.mistral.ai/api-keys",
            "deepseek" => "https://platform.deepseek.com/api_keys",
            "together" => "https://api.together.xyz/settings/api-keys",
            "fireworks" => "https://fireworks.ai/account/api-keys",
            "perplexity" => "https://www.perplexity.ai/settings/api",
            "xai" => "https://console.x.ai",
            "cohere" => "https://dashboard.cohere.com/api-keys",
            "moonshot" => "https://platform.moonshot.cn/console/api-keys",
            "minimax" => "https://www.minimaxi.com/user-center/basic-information",
            "vercel" => "https://vercel.com/account/tokens",
            "cloudflare" => "https://dash.cloudflare.com/profile/api-tokens",
            "bedrock" => "https://console.aws.amazon.com/iam",
            "gemini" | "google" | "google-gemini" => "https://aistudio.google.com/app/apikey",
            _ => "",
        };

        println!();
        if !key_url.is_empty() {
            print_bullet(&t!("onboard.provider.api_key_url", url = ui::url(key_url)));
        }
        print_bullet(&t!("onboard.provider.api_key_later"));
        println!();

        let key: String = Input::new()
            .with_prompt(format!("  {}", t!("onboard.provider.paste_key")))
            .allow_empty(true)
            .interact_text()?;

        if key.is_empty() {
            let env_var = provider_env_var(provider_name);
            print_bullet(&t!(
                "onboard.provider.key_skipped",
                env_var = ui::yellow(env_var)
            ));
        }

        key
    };

    // ── Model selection ──
    let models: Vec<(&str, &str)> = match provider_name {
        "openrouter" => vec![
            (
                "anthropic/claude-sonnet-4-6",
                "Claude Sonnet 4.6 (balanced, recommended)",
            ),
            (
                "anthropic/claude-opus-4-6",
                "Claude Opus 4.6 (most capable)",
            ),
            ("openai/gpt-5.2", "GPT-5.2 (OpenAI flagship)"),
            ("openai/gpt-5-mini", "GPT-5 Mini (fast, cheap)"),
            ("google/gemini-2.5-flash", "Gemini 2.5 Flash (Google, fast)"),
            (
                "meta-llama/llama-3.3-70b-instruct",
                "Llama 3.3 70B (open source)",
            ),
            ("deepseek/deepseek-chat", "DeepSeek V3.2 (affordable)"),
        ],
        "anthropic" => vec![
            (
                "claude-sonnet-4-6",
                "Claude Sonnet 4.6 (balanced, recommended)",
            ),
            ("claude-opus-4-6", "Claude Opus 4.6 (most capable)"),
            ("claude-haiku-4-5", "Claude Haiku 4.5 (fastest, cheapest)"),
        ],
        "openai" => vec![
            ("gpt-5.2", "GPT-5.2 (flagship)"),
            ("gpt-5-mini", "GPT-5 Mini (fast, cheap)"),
            ("gpt-4.1", "GPT-4.1 (1M context, non-reasoning)"),
        ],
        "venice" => vec![
            ("deepseek-v3.2", "DeepSeek V3.2 (recommended)"),
            (
                "claude-opus-4-6",
                "Claude Opus 4.6 via Venice (most capable)",
            ),
            ("llama-3.3-70b", "Llama 3.3 70B (open source, fast)"),
        ],
        "groq" => vec![
            (
                "llama-3.3-70b-versatile",
                "Llama 3.3 70B (fast, recommended)",
            ),
            ("llama-3.1-8b-instant", "Llama 3.1 8B (instant)"),
            ("openai/gpt-oss-120b", "GPT-OSS 120B (OpenAI open-weight)"),
        ],
        "mistral" => vec![
            ("mistral-large-2512", "Mistral Large 3 (flagship)"),
            ("codestral-2508", "Codestral (code-focused)"),
            ("mistral-small-2506", "Mistral Small 3.2 (fast, cheap)"),
        ],
        "deepseek" => vec![
            ("deepseek-chat", "DeepSeek Chat (V3.2, recommended)"),
            ("deepseek-reasoner", "DeepSeek Reasoner (V3.2 thinking)"),
        ],
        "xai" => vec![
            ("grok-4-0709", "Grok 4 (flagship)"),
            ("grok-3-mini", "Grok 3 Mini (fast, cheap)"),
        ],
        "perplexity" => vec![
            ("sonar-pro", "Sonar Pro (search + reasoning)"),
            ("sonar", "Sonar (search, fast)"),
            (
                "sonar-deep-research",
                "Sonar Deep Research (expert research)",
            ),
        ],
        "fireworks" => vec![
            (
                "accounts/fireworks/models/deepseek-v3p2",
                "DeepSeek V3.2 (recommended)",
            ),
            (
                "accounts/fireworks/models/llama-v3p3-70b-instruct",
                "Llama 3.3 70B",
            ),
            (
                "accounts/fireworks/models/qwen3-235b-a22b",
                "Qwen3 235B (code-optimized)",
            ),
        ],
        "together" => vec![
            (
                "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                "Llama 3.3 70B Turbo (recommended)",
            ),
            (
                "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
                "Llama 3.1 8B Turbo (fast)",
            ),
            ("deepseek-ai/DeepSeek-V3.1", "DeepSeek V3.1"),
        ],
        "cohere" => vec![
            ("command-a-03-2025", "Command A (flagship)"),
            ("command-r7b-12-2024", "Command R 7B (fast)"),
        ],
        "moonshot" => vec![
            ("kimi-k2.5", "Kimi K2.5 (flagship, multimodal)"),
            ("kimi-k2-turbo-preview", "Kimi K2 Turbo (fast)"),
        ],
        "glm" => vec![
            ("glm-4.7", "GLM-4.7 (flagship)"),
            ("glm-4.7-flash", "GLM-4.7 Flash (fast)"),
        ],
        "minimax" => vec![
            ("MiniMax-M2.1", "MiniMax M2.1 (flagship)"),
            ("MiniMax-M2", "MiniMax M2"),
        ],
        "ollama" => vec![
            ("llama3.2", "Llama 3.2 (small, recommended local)"),
            ("llama3.3", "Llama 3.3 70B (best quality local)"),
            ("phi4", "Phi-4 14B (Microsoft, strong reasoning)"),
            ("qwen3", "Qwen3 (multilingual, hybrid thinking)"),
        ],
        "gemini" | "google" | "google-gemini" => vec![
            ("gemini-2.5-flash", "Gemini 2.5 Flash (fast, recommended)"),
            ("gemini-2.5-pro", "Gemini 2.5 Pro (best quality)"),
            ("gemini-2.5-flash-lite", "Gemini 2.5 Flash Lite (cheapest)"),
        ],
        _ => vec![("default", "Default model")],
    };

    let model_labels: Vec<&str> = models.iter().map(|(_, label)| *label).collect();

    let model_idx = Select::new()
        .with_prompt(format!("  {}", t!("onboard.provider.select_model")))
        .items(&model_labels)
        .default(0)
        .interact()?;

    let model = models[model_idx].0.to_string();

    println!(
        "  {} {}",
        ui::success("✓"),
        t!(
            "onboard.provider.confirm",
            provider = ui::value(provider_name),
            model = ui::value(&model)
        )
    );

    Ok((provider_name.to_string(), api_key, model))
}
