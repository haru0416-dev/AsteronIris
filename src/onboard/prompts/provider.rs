use anyhow::Result;
use console::style;
use dialoguer::{Input, Select};

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
            style(t!("onboard.provider.custom_title")).white().bold(),
            style(format!("— {}", t!("onboard.provider.custom_subtitle"))).dim()
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
            style("✓").green().bold(),
            t!(
                "onboard.provider.confirm",
                provider = style(&provider_name).green(),
                model = style(&model).green()
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
        if crate::providers::gemini::GeminiProvider::has_cli_credentials() {
            print_bullet(&format!(
                "{} {}",
                style("✓").green().bold(),
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
                    style("✓").green().bold(),
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
                style("✓").green().bold(),
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
            print_bullet(&t!(
                "onboard.provider.api_key_url",
                url = style(key_url).cyan().underlined()
            ));
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
                env_var = style(env_var).yellow()
            ));
        }

        key
    };

    // ── Model selection ──
    let models: Vec<(&str, &str)> = match provider_name {
        "openrouter" => vec![
            (
                "anthropic/claude-sonnet-4-20250514",
                "Claude Sonnet 4 (balanced, recommended)",
            ),
            (
                "anthropic/claude-3.5-sonnet",
                "Claude 3.5 Sonnet (fast, affordable)",
            ),
            ("openai/gpt-4o", "GPT-4o (OpenAI flagship)"),
            ("openai/gpt-4o-mini", "GPT-4o Mini (fast, cheap)"),
            (
                "google/gemini-2.0-flash-001",
                "Gemini 2.0 Flash (Google, fast)",
            ),
            (
                "meta-llama/llama-3.3-70b-instruct",
                "Llama 3.3 70B (open source)",
            ),
            ("deepseek/deepseek-chat", "DeepSeek Chat (affordable)"),
        ],
        "anthropic" => vec![
            (
                "claude-sonnet-4-20250514",
                "Claude Sonnet 4 (balanced, recommended)",
            ),
            ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet (fast)"),
            (
                "claude-3-5-haiku-20241022",
                "Claude 3.5 Haiku (fastest, cheapest)",
            ),
        ],
        "openai" => vec![
            ("gpt-4o", "GPT-4o (flagship)"),
            ("gpt-4o-mini", "GPT-4o Mini (fast, cheap)"),
            ("o1-mini", "o1-mini (reasoning)"),
        ],
        "venice" => vec![
            ("llama-3.3-70b", "Llama 3.3 70B (default, fast)"),
            ("claude-opus-45", "Claude Opus 4.5 via Venice (strongest)"),
            ("llama-3.1-405b", "Llama 3.1 405B (largest open source)"),
        ],
        "groq" => vec![
            (
                "llama-3.3-70b-versatile",
                "Llama 3.3 70B (fast, recommended)",
            ),
            ("llama-3.1-8b-instant", "Llama 3.1 8B (instant)"),
            ("mixtral-8x7b-32768", "Mixtral 8x7B (32K context)"),
        ],
        "mistral" => vec![
            ("mistral-large-latest", "Mistral Large (flagship)"),
            ("codestral-latest", "Codestral (code-focused)"),
            ("mistral-small-latest", "Mistral Small (fast, cheap)"),
        ],
        "deepseek" => vec![
            ("deepseek-chat", "DeepSeek Chat (V3, recommended)"),
            ("deepseek-reasoner", "DeepSeek Reasoner (R1)"),
        ],
        "xai" => vec![
            ("grok-3", "Grok 3 (flagship)"),
            ("grok-3-mini", "Grok 3 Mini (fast)"),
        ],
        "perplexity" => vec![
            ("sonar-pro", "Sonar Pro (search + reasoning)"),
            ("sonar", "Sonar (search, fast)"),
        ],
        "fireworks" => vec![
            (
                "accounts/fireworks/models/llama-v3p3-70b-instruct",
                "Llama 3.3 70B",
            ),
            (
                "accounts/fireworks/models/mixtral-8x22b-instruct",
                "Mixtral 8x22B",
            ),
        ],
        "together" => vec![
            (
                "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo",
                "Llama 3.1 70B Turbo",
            ),
            (
                "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
                "Llama 3.1 8B Turbo",
            ),
            ("mistralai/Mixtral-8x22B-Instruct-v0.1", "Mixtral 8x22B"),
        ],
        "cohere" => vec![
            ("command-r-plus", "Command R+ (flagship)"),
            ("command-r", "Command R (fast)"),
        ],
        "moonshot" => vec![
            ("moonshot-v1-128k", "Moonshot V1 128K"),
            ("moonshot-v1-32k", "Moonshot V1 32K"),
        ],
        "glm" => vec![
            ("glm-4-plus", "GLM-4 Plus (flagship)"),
            ("glm-4-flash", "GLM-4 Flash (fast)"),
        ],
        "minimax" => vec![
            ("abab6.5s-chat", "ABAB 6.5s Chat"),
            ("abab6.5-chat", "ABAB 6.5 Chat"),
        ],
        "ollama" => vec![
            ("llama3.2", "Llama 3.2 (recommended local)"),
            ("mistral", "Mistral 7B"),
            ("codellama", "Code Llama"),
            ("phi3", "Phi-3 (small, fast)"),
        ],
        "gemini" | "google" | "google-gemini" => vec![
            ("gemini-2.0-flash", "Gemini 2.0 Flash (fast, recommended)"),
            (
                "gemini-2.0-flash-lite",
                "Gemini 2.0 Flash Lite (fastest, cheapest)",
            ),
            ("gemini-1.5-pro", "Gemini 1.5 Pro (best quality)"),
            ("gemini-1.5-flash", "Gemini 1.5 Flash (balanced)"),
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
        style("✓").green().bold(),
        t!(
            "onboard.provider.confirm",
            provider = style(provider_name).green(),
            model = style(&model).green()
        )
    );

    Ok((provider_name.to_string(), api_key, model))
}
