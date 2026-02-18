use crate::config::schema::{IrcConfig, WhatsAppConfig};
use crate::config::{
    ChannelsConfig, ComposioConfig, DiscordConfig, IMessageConfig, MatrixConfig, MemoryConfig,
    SecretsConfig, SlackConfig, TelegramConfig, WebhookConfig,
};
use anyhow::{Context, Result};
use console::style;
use dialoguer::{Confirm, Input, Select};
use std::fs;
use std::path::PathBuf;

use super::domain::{provider_env_var, validate_base_url, validate_non_empty};
use super::view::print_bullet;

#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub user_name: String,
    pub timezone: String,
    pub agent_name: String,
    pub communication_style: String,
}

pub fn setup_workspace() -> Result<(PathBuf, PathBuf)> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;
    let default_dir = home.join(".asteroniris");

    print_bullet(&format!(
        "Default location: {}",
        style(default_dir.display()).green()
    ));

    let use_default = Confirm::new()
        .with_prompt("  Use default workspace location?")
        .default(true)
        .interact()?;

    let asteroniris_dir = if use_default {
        default_dir
    } else {
        let custom = input_workspace_path()?;
        PathBuf::from(custom)
    };

    let workspace_dir = asteroniris_dir.join("workspace");
    let config_path = asteroniris_dir.join("config.toml");

    fs::create_dir_all(&workspace_dir).context("Failed to create workspace directory")?;

    println!(
        "  {} Workspace: {}",
        style("✓").green().bold(),
        style(workspace_dir.display()).green()
    );

    Ok((workspace_dir, config_path))
}

// ── Step 2: Provider & API Key ───────────────────────────────────

#[allow(clippy::too_many_lines)]
pub fn setup_provider() -> Result<(String, String, String)> {
    // ── Tier selection ──
    let tiers = vec![
        "⭐ Recommended (OpenRouter, Venice, Anthropic, OpenAI, Gemini)",
        "⚡ Fast inference (Groq, Fireworks, Together AI)",
        "🌐 Gateway / proxy (Vercel AI, Cloudflare AI, Amazon Bedrock)",
        "🔬 Specialized (Moonshot/Kimi, GLM/Zhipu, MiniMax, Qianfan, Z.AI, Synthetic, OpenCode Zen, Cohere)",
        "🏠 Local / private (Ollama — no API key needed)",
        "🔧 Custom — bring your own OpenAI-compatible API",
    ];

    let tier_idx = Select::new()
        .with_prompt("  Select provider category")
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
            style("Custom Provider Setup").white().bold(),
            style("— any OpenAI-compatible API").dim()
        );
        print_bullet(
            "AsteronIris works with ANY API that speaks the OpenAI chat completions format.",
        );
        print_bullet("Examples: LiteLLM, LocalAI, vLLM, text-generation-webui, LM Studio, etc.");
        println!();

        let base_url: String = Input::new()
            .with_prompt("  API base URL (e.g. http://localhost:1234 or https://my-api.com)")
            .interact_text()?;

        let base_url = validate_base_url(&base_url)?;

        let api_key: String = Input::new()
            .with_prompt("  API key (or Enter to skip if not needed)")
            .allow_empty(true)
            .interact_text()?;

        let model: String = Input::new()
            .with_prompt("  Model name (e.g. llama3, gpt-4o, mistral)")
            .default("default".into())
            .interact_text()?;

        let provider_name = format!("custom:{base_url}");

        println!(
            "  {} Provider: {} | Model: {}",
            style("✓").green().bold(),
            style(&provider_name).green(),
            style(&model).green()
        );

        return Ok((provider_name, api_key, model));
    }

    let provider_labels: Vec<&str> = providers.iter().map(|(_, label)| *label).collect();

    let provider_idx = Select::new()
        .with_prompt("  Select your AI provider")
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let provider_name = providers[provider_idx].0;

    // ── API key ──
    let api_key = if provider_name == "ollama" {
        print_bullet("Ollama runs locally — no API key needed!");
        String::new()
    } else if provider_name == "gemini"
        || provider_name == "google"
        || provider_name == "google-gemini"
    {
        // Special handling for Gemini: check for CLI auth first
        if crate::providers::gemini::GeminiProvider::has_cli_credentials() {
            print_bullet(&format!(
                "{} Gemini CLI credentials detected! You can skip the API key.",
                style("✓").green().bold()
            ));
            print_bullet("AsteronIris will reuse your existing Gemini CLI authentication.");
            println!();

            let use_cli: bool = dialoguer::Confirm::new()
                .with_prompt("  Use existing Gemini CLI authentication?")
                .default(true)
                .interact()?;

            if use_cli {
                println!(
                    "  {} Using Gemini CLI OAuth tokens",
                    style("✓").green().bold()
                );
                String::new() // Empty key = will use CLI tokens
            } else {
                print_bullet("Get your API key at: https://aistudio.google.com/app/apikey");
                Input::new()
                    .with_prompt("  Paste your Gemini API key")
                    .allow_empty(true)
                    .interact_text()?
            }
        } else if std::env::var("GEMINI_API_KEY").is_ok() {
            print_bullet(&format!(
                "{} GEMINI_API_KEY environment variable detected!",
                style("✓").green().bold()
            ));
            String::new()
        } else {
            print_bullet("Get your API key at: https://aistudio.google.com/app/apikey");
            print_bullet("Or run `gemini` CLI to authenticate (tokens will be reused).");
            println!();

            Input::new()
                .with_prompt("  Paste your Gemini API key (or press Enter to skip)")
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
            print_bullet(&format!(
                "Get your API key at: {}",
                style(key_url).cyan().underlined()
            ));
        }
        print_bullet("You can also set it later via env var or config file.");
        println!();

        let key: String = Input::new()
            .with_prompt("  Paste your API key (or press Enter to skip)")
            .allow_empty(true)
            .interact_text()?;

        if key.is_empty() {
            let env_var = provider_env_var(provider_name);
            print_bullet(&format!(
                "Skipped. Set {} or edit config.toml later.",
                style(env_var).yellow()
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
        .with_prompt("  Select your default model")
        .items(&model_labels)
        .default(0)
        .interact()?;

    let model = models[model_idx].0.to_string();

    println!(
        "  {} Provider: {} | Model: {}",
        style("✓").green().bold(),
        style(provider_name).green(),
        style(&model).green()
    );

    Ok((provider_name.to_string(), api_key, model))
}

// ── Step 5: Tool Mode & Security ────────────────────────────────

pub fn setup_tool_mode() -> Result<(ComposioConfig, SecretsConfig)> {
    print_bullet("Choose how AsteronIris connects to external apps.");
    print_bullet("You can always change this later in config.toml.");
    println!();

    let options = vec![
        "Sovereign (local only) — you manage API keys, full privacy (default)",
        "Composio (managed OAuth) — 1000+ apps via OAuth, no raw keys shared",
    ];

    let choice = Select::new()
        .with_prompt("  Select tool mode")
        .items(&options)
        .default(0)
        .interact()?;

    let composio_config = if choice == 1 {
        println!();
        println!(
            "  {} {}",
            style("Composio Setup").white().bold(),
            style("— 1000+ OAuth integrations (Gmail, Notion, GitHub, Slack, ...)").dim()
        );
        print_bullet("Get your API key at: https://app.composio.dev/settings");
        print_bullet("AsteronIris uses Composio as a tool — your core agent stays local.");
        println!();

        let api_key: String = Input::new()
            .with_prompt("  Composio API key (or Enter to skip)")
            .allow_empty(true)
            .interact_text()?;

        if api_key.trim().is_empty() {
            println!(
                "  {} Skipped — set composio.api_key in config.toml later",
                style("→").dim()
            );
            ComposioConfig::default()
        } else {
            println!(
                "  {} Composio: {} (1000+ OAuth tools available)",
                style("✓").green().bold(),
                style("enabled").green()
            );
            ComposioConfig {
                enabled: true,
                api_key: Some(api_key),
                ..ComposioConfig::default()
            }
        }
    } else {
        println!(
            "  {} Tool mode: {} — full privacy, you own every key",
            style("✓").green().bold(),
            style("Sovereign (local only)").green()
        );
        ComposioConfig::default()
    };

    // ── Encrypted secrets ──
    println!();
    print_bullet("AsteronIris can encrypt API keys stored in config.toml.");
    print_bullet("A local key file protects against plaintext exposure and accidental leaks.");

    let encrypt = Confirm::new()
        .with_prompt("  Enable encrypted secret storage?")
        .default(true)
        .interact()?;

    let secrets_config = SecretsConfig { encrypt };

    if encrypt {
        println!(
            "  {} Secrets: {} — keys encrypted with local key file",
            style("✓").green().bold(),
            style("encrypted").green()
        );
    } else {
        println!(
            "  {} Secrets: {} — keys stored as plaintext (not recommended)",
            style("✓").green().bold(),
            style("plaintext").yellow()
        );
    }

    Ok((composio_config, secrets_config))
}

// ── Step 6: Project Context ─────────────────────────────────────

pub fn setup_project_context() -> Result<ProjectContext> {
    print_bullet("Let's personalize your agent. You can always update these later.");
    print_bullet("Press Enter to accept defaults.");
    println!();

    let user_name: String = Input::new()
        .with_prompt("  Your name")
        .default("User".into())
        .interact_text()?;

    let tz_options = vec![
        "US/Eastern (EST/EDT)",
        "US/Central (CST/CDT)",
        "US/Mountain (MST/MDT)",
        "US/Pacific (PST/PDT)",
        "Europe/London (GMT/BST)",
        "Europe/Berlin (CET/CEST)",
        "Asia/Tokyo (JST)",
        "UTC",
        "Other (type manually)",
    ];

    let tz_idx = Select::new()
        .with_prompt("  Your timezone")
        .items(&tz_options)
        .default(0)
        .interact()?;

    let timezone = if tz_idx == tz_options.len() - 1 {
        Input::new()
            .with_prompt("  Enter timezone (e.g. America/New_York)")
            .default("UTC".into())
            .interact_text()?
    } else {
        // Extract the short label before the parenthetical
        tz_options[tz_idx]
            .split('(')
            .next()
            .unwrap_or("UTC")
            .trim()
            .to_string()
    };

    let agent_name: String = Input::new()
        .with_prompt("  Agent name")
        .default("AsteronIris".into())
        .interact_text()?;

    let style_options = vec![
        "Direct & concise — skip pleasantries, get to the point",
        "Friendly & casual — warm, human, and helpful",
        "Professional & polished — calm, confident, and clear",
        "Expressive & playful — more personality + natural emojis",
        "Technical & detailed — thorough explanations, code-first",
        "Balanced — adapt to the situation",
        "Custom — write your own style guide",
    ];

    let style_idx = Select::new()
        .with_prompt("  Communication style")
        .items(&style_options)
        .default(1)
        .interact()?;

    let communication_style = match style_idx {
        0 => "Be direct and concise. Skip pleasantries. Get to the point.".to_string(),
        1 => "Be friendly, human, and conversational. Show warmth and empathy while staying efficient. Use natural contractions.".to_string(),
        2 => "Be professional and polished. Stay calm, structured, and respectful. Use occasional tone-setting emojis only when appropriate.".to_string(),
        3 => "Be expressive and playful when appropriate. Use relevant emojis naturally (0-2 max), and keep serious topics emoji-light.".to_string(),
        4 => "Be technical and detailed. Thorough explanations, code-first.".to_string(),
        5 => "Adapt to the situation. Default to warm and clear communication; be concise when needed, thorough when it matters.".to_string(),
        _ => Input::new()
            .with_prompt("  Custom communication style")
            .default(
                "Be warm, natural, and clear. Use occasional relevant emojis (1-2 max) and avoid robotic phrasing.".into(),
            )
            .interact_text()?,
    };

    println!(
        "  {} Context: {} | {} | {} | {}",
        style("✓").green().bold(),
        style(&user_name).green(),
        style(&timezone).green(),
        style(&agent_name).green(),
        style(&communication_style).green().dim()
    );

    Ok(ProjectContext {
        user_name,
        timezone,
        agent_name,
        communication_style,
    })
}

// ── Step 6: Memory Configuration ───────────────────────────────

pub fn setup_memory() -> Result<MemoryConfig> {
    print_bullet("Choose how AsteronIris stores and searches memories.");
    print_bullet("You can always change this later in config.toml.");
    println!();

    let options = vec![
        "SQLite with Vector Search (recommended) — fast, hybrid search, embeddings",
        "Markdown Files — simple, human-readable, no dependencies",
        "None — disable persistent memory",
    ];

    let choice = Select::new()
        .with_prompt("  Select memory backend")
        .items(&options)
        .default(0)
        .interact()?;

    let backend = match choice {
        1 => "markdown",
        2 => "none",
        _ => "sqlite", // 0 and any unexpected value defaults to sqlite
    };

    let auto_save = if backend == "none" {
        false
    } else {
        let save = Confirm::new()
            .with_prompt("  Auto-save conversations to memory?")
            .default(true)
            .interact()?;
        save
    };

    println!(
        "  {} Memory: {} (auto-save: {})",
        style("✓").green().bold(),
        style(backend).green(),
        if auto_save { "on" } else { "off" }
    );

    Ok(MemoryConfig {
        backend: backend.to_string(),
        auto_save,
        hygiene_enabled: backend == "sqlite", // Only enable hygiene for SQLite
        archive_after_days: if backend == "sqlite" { 7 } else { 0 },
        purge_after_days: if backend == "sqlite" { 30 } else { 0 },
        conversation_retention_days: 30,
        layer_retention_working_days: None,
        layer_retention_episodic_days: None,
        layer_retention_semantic_days: None,
        layer_retention_procedural_days: None,
        layer_retention_identity_days: None,
        ledger_retention_days: None,
        embedding_provider: "none".to_string(),
        embedding_model: "text-embedding-3-small".to_string(),
        embedding_dimensions: 1536,
        vector_weight: 0.7,
        keyword_weight: 0.3,
        embedding_cache_size: if backend == "sqlite" { 10000 } else { 0 },
        chunk_max_tokens: 512,
    })
}

// ── Step 3: Channels ────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub fn setup_channels() -> Result<ChannelsConfig> {
    print_bullet("Channels let you talk to AsteronIris from anywhere.");
    print_bullet("CLI is always available. Connect more channels now.");
    println!();

    let mut config = ChannelsConfig {
        cli: true,
        telegram: None,
        discord: None,
        slack: None,
        webhook: None,
        imessage: None,
        matrix: None,
        whatsapp: None,
        email: None,
        irc: None,
    };

    loop {
        let options = vec![
            format!(
                "Telegram   {}",
                if config.telegram.is_some() {
                    "✅ connected"
                } else {
                    "— connect your bot"
                }
            ),
            format!(
                "Discord    {}",
                if config.discord.is_some() {
                    "✅ connected"
                } else {
                    "— connect your bot"
                }
            ),
            format!(
                "Slack      {}",
                if config.slack.is_some() {
                    "✅ connected"
                } else {
                    "— connect your bot"
                }
            ),
            format!(
                "iMessage   {}",
                if config.imessage.is_some() {
                    "✅ configured"
                } else {
                    "— macOS only"
                }
            ),
            format!(
                "Matrix     {}",
                if config.matrix.is_some() {
                    "✅ connected"
                } else {
                    "— self-hosted chat"
                }
            ),
            format!(
                "WhatsApp   {}",
                if config.whatsapp.is_some() {
                    "✅ connected"
                } else {
                    "— Business Cloud API"
                }
            ),
            format!(
                "IRC        {}",
                if config.irc.is_some() {
                    "✅ configured"
                } else {
                    "— IRC over TLS"
                }
            ),
            format!(
                "Webhook    {}",
                if config.webhook.is_some() {
                    "✅ configured"
                } else {
                    "— HTTP endpoint"
                }
            ),
            "Done — finish setup".to_string(),
        ];

        let choice = Select::new()
            .with_prompt("  Connect a channel (or Done to continue)")
            .items(&options)
            .default(8)
            .interact()?;

        match choice {
            0 => {
                // ── Telegram ──
                println!();
                println!(
                    "  {} {}",
                    style("Telegram Setup").white().bold(),
                    style("— talk to AsteronIris from Telegram").dim()
                );
                print_bullet("1. Open Telegram and message @BotFather");
                print_bullet("2. Send /newbot and follow the prompts");
                print_bullet("3. Copy the bot token and paste it below");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot token (from @BotFather)")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                let url = format!("https://api.telegram.org/bot{token}/getMe");
                match client.get(&url).send() {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let bot_name = data
                            .get("result")
                            .and_then(|r| r.get("username"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        println!(
                            "\r  {} Connected as @{bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token and try again",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                print_bullet(
                    "Allowlist your own Telegram identity first (recommended for secure + fast setup).",
                );
                print_bullet(
                    "Use your @username without '@' (example: argenis), or your numeric Telegram user ID.",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Telegram identities (comma-separated: username without '@' and/or numeric user ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Telegram inbound messages will be denied until you add your username/user ID or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.telegram = Some(TelegramConfig {
                    bot_token: token,
                    allowed_users,
                });
            }
            1 => {
                // ── Discord ──
                println!();
                println!(
                    "  {} {}",
                    style("Discord Setup").white().bold(),
                    style("— talk to AsteronIris from Discord").dim()
                );
                print_bullet("1. Go to https://discord.com/developers/applications");
                print_bullet("2. Create a New Application → Bot → Copy token");
                print_bullet("3. Enable MESSAGE CONTENT intent under Bot settings");
                print_bullet("4. Invite bot to your server with messages permission");
                println!();

                let token: String = Input::new().with_prompt("  Bot token").interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .get("https://discord.com/api/v10/users/@me")
                    .header("Authorization", format!("Bot {token}"))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let bot_name = data
                            .get("username")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        println!(
                            "\r  {} Connected as {bot_name}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token and try again",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let guild: String = Input::new()
                    .with_prompt("  Server (guild) ID (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("Allowlist your own Discord user ID first (recommended).");
                print_bullet(
                    "Get it in Discord: Settings -> Advanced -> Developer Mode (ON), then right-click your profile -> Copy User ID.",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Discord user IDs (comma-separated, recommended: your own ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Discord inbound messages will be denied until you add IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.discord = Some(DiscordConfig {
                    bot_token: token,
                    guild_id: if guild.is_empty() { None } else { Some(guild) },
                    allowed_users,
                });
            }
            2 => {
                // ── Slack ──
                println!();
                println!(
                    "  {} {}",
                    style("Slack Setup").white().bold(),
                    style("— talk to AsteronIris from Slack").dim()
                );
                print_bullet("1. Go to https://api.slack.com/apps → Create New App");
                print_bullet("2. Add Bot Token Scopes: chat:write, channels:history");
                print_bullet("3. Install to workspace and copy the Bot Token");
                println!();

                let token: String = Input::new()
                    .with_prompt("  Bot token (xoxb-...)")
                    .interact_text()?;

                if token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .get("https://slack.com/api/auth.test")
                    .bearer_auth(&token)
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let ok = data
                            .get("ok")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(false);
                        let team = data
                            .get("team")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        if ok {
                            println!(
                                "\r  {} Connected to workspace: {team}        ",
                                style("✅").green().bold()
                            );
                        } else {
                            let err = data
                                .get("error")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("unknown error");
                            println!("\r  {} Slack error: {err}", style("❌").red().bold());
                            continue;
                        }
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check your token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let app_token: String = Input::new()
                    .with_prompt("  App token (xapp-..., optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                let channel: String = Input::new()
                    .with_prompt("  Default channel ID (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                print_bullet("Allowlist your own Slack member ID first (recommended).");
                print_bullet(
                    "Member IDs usually start with 'U' (open your Slack profile -> More -> Copy member ID).",
                );
                print_bullet("Use '*' only for temporary open testing.");

                let allowed_users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed Slack user IDs (comma-separated, recommended: your own member ID, '*' for all)",
                    )
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if allowed_users_str.trim().is_empty() {
                    vec![]
                } else {
                    allowed_users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    println!(
                        "  {} No users allowlisted — Slack inbound messages will be denied until you add IDs or '*'.",
                        style("⚠").yellow().bold()
                    );
                }

                config.slack = Some(SlackConfig {
                    bot_token: token,
                    app_token: if app_token.is_empty() {
                        None
                    } else {
                        Some(app_token)
                    },
                    channel_id: if channel.is_empty() {
                        None
                    } else {
                        Some(channel)
                    },
                    allowed_users,
                });
            }
            3 => {
                // ── iMessage ──
                println!();
                println!(
                    "  {} {}",
                    style("iMessage Setup").white().bold(),
                    style("— macOS only, reads from Messages.app").dim()
                );

                if !cfg!(target_os = "macos") {
                    println!(
                        "  {} iMessage is only available on macOS.",
                        style("⚠").yellow().bold()
                    );
                    continue;
                }

                print_bullet(
                    "AsteronIris reads your iMessage database and replies via AppleScript.",
                );
                print_bullet(
                    "You need to grant Full Disk Access to your terminal in System Settings.",
                );
                println!();

                let contacts_str: String = Input::new()
                    .with_prompt("  Allowed contacts (comma-separated phone/email, or * for all)")
                    .default("*".into())
                    .interact_text()?;

                let allowed_contacts = if contacts_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    contacts_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .collect()
                };

                config.imessage = Some(IMessageConfig { allowed_contacts });
                println!(
                    "  {} iMessage configured (contacts: {})",
                    style("✅").green().bold(),
                    style(&contacts_str).cyan()
                );
            }
            4 => {
                // ── Matrix ──
                println!();
                println!(
                    "  {} {}",
                    style("Matrix Setup").white().bold(),
                    style("— self-hosted, federated chat").dim()
                );
                print_bullet("You need a Matrix account and an access token.");
                print_bullet("Get a token via Element → Settings → Help & About → Access Token.");
                println!();

                let homeserver: String = Input::new()
                    .with_prompt("  Homeserver URL (e.g. https://matrix.org)")
                    .interact_text()?;

                if homeserver.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let access_token: String =
                    Input::new().with_prompt("  Access token").interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} Skipped — token required", style("→").dim());
                    continue;
                }

                // Test connection
                let hs = homeserver.trim_end_matches('/');
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                match client
                    .get(format!("{hs}/_matrix/client/v3/account/whoami"))
                    .header("Authorization", format!("Bearer {access_token}"))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().unwrap_or_default();
                        let user_id = data
                            .get("user_id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("unknown");
                        println!(
                            "\r  {} Connected as {user_id}        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check homeserver URL and token",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let room_id: String = Input::new()
                    .with_prompt("  Room ID (e.g. !abc123:matrix.org)")
                    .interact_text()?;

                let users_str: String = Input::new()
                    .with_prompt("  Allowed users (comma-separated @user:server, or * for all)")
                    .default("*".into())
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.matrix = Some(MatrixConfig {
                    homeserver: homeserver.trim_end_matches('/').to_string(),
                    access_token,
                    room_id,
                    allowed_users,
                });
            }
            5 => {
                // ── WhatsApp ──
                println!();
                println!(
                    "  {} {}",
                    style("WhatsApp Setup").white().bold(),
                    style("— Business Cloud API").dim()
                );
                print_bullet("1. Go to developers.facebook.com and create a WhatsApp app");
                print_bullet("2. Add the WhatsApp product and get your phone number ID");
                print_bullet("3. Generate a temporary access token (System User)");
                print_bullet("4. Configure webhook URL to: https://your-domain/whatsapp");
                println!();

                let access_token: String = Input::new()
                    .with_prompt("  Access token (from Meta Developers)")
                    .interact_text()?;

                if access_token.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let phone_number_id: String = Input::new()
                    .with_prompt("  Phone number ID (from WhatsApp app settings)")
                    .interact_text()?;

                if phone_number_id.trim().is_empty() {
                    println!("  {} Skipped — phone number ID required", style("→").dim());
                    continue;
                }

                let verify_token: String = Input::new()
                    .with_prompt("  Webhook verify token (create your own)")
                    .default("asteroniris-whatsapp-verify".into())
                    .interact_text()?;

                // Test connection
                print!("  {} Testing connection... ", style("⏳").dim());
                let client = reqwest::blocking::Client::new();
                let url = format!(
                    "https://graph.facebook.com/v18.0/{}",
                    phone_number_id.trim()
                );
                match client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", access_token.trim()))
                    .send()
                {
                    Ok(resp) if resp.status().is_success() => {
                        println!(
                            "\r  {} Connected to WhatsApp API        ",
                            style("✅").green().bold()
                        );
                    }
                    _ => {
                        println!(
                            "\r  {} Connection failed — check access token and phone number ID",
                            style("❌").red().bold()
                        );
                        continue;
                    }
                }

                let users_str: String = Input::new()
                    .with_prompt(
                        "  Allowed phone numbers (comma-separated +1234567890, or * for all)",
                    )
                    .default("*".into())
                    .interact_text()?;

                let allowed_numbers = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str.split(',').map(|s| s.trim().to_string()).collect()
                };

                config.whatsapp = Some(WhatsAppConfig {
                    access_token: access_token.trim().to_string(),
                    phone_number_id: phone_number_id.trim().to_string(),
                    verify_token: verify_token.trim().to_string(),
                    allowed_numbers,
                    app_secret: None, // Can be set via ASTERONIRIS_WHATSAPP_APP_SECRET env var
                });
            }
            6 => {
                // ── IRC ──
                println!();
                println!(
                    "  {} {}",
                    style("IRC Setup").white().bold(),
                    style("— IRC over TLS").dim()
                );
                print_bullet("IRC connects over TLS to any IRC server");
                print_bullet("Supports SASL PLAIN and NickServ authentication");
                println!();

                let server: String = Input::new()
                    .with_prompt("  IRC server (hostname)")
                    .interact_text()?;

                if server.trim().is_empty() {
                    println!("  {} Skipped", style("→").dim());
                    continue;
                }

                let port_str: String = Input::new()
                    .with_prompt("  Port")
                    .default("6697".into())
                    .interact_text()?;

                let port: u16 = if let Ok(p) = port_str.trim().parse() {
                    p
                } else {
                    println!("  {} Invalid port, using 6697", style("→").dim());
                    6697
                };

                let nickname: String =
                    Input::new().with_prompt("  Bot nickname").interact_text()?;

                if nickname.trim().is_empty() {
                    println!("  {} Skipped — nickname required", style("→").dim());
                    continue;
                }

                let channels_str: String = Input::new()
                    .with_prompt("  Channels to join (comma-separated: #channel1,#channel2)")
                    .allow_empty(true)
                    .interact_text()?;

                let channels = if channels_str.trim().is_empty() {
                    vec![]
                } else {
                    channels_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                print_bullet(
                    "Allowlist nicknames that can interact with the bot (case-insensitive).",
                );
                print_bullet("Use '*' to allow anyone (not recommended for production).");

                let users_str: String = Input::new()
                    .with_prompt("  Allowed nicknames (comma-separated, or * for all)")
                    .allow_empty(true)
                    .interact_text()?;

                let allowed_users = if users_str.trim() == "*" {
                    vec!["*".into()]
                } else {
                    users_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                };

                if allowed_users.is_empty() {
                    print_bullet(
                        "⚠️  Empty allowlist — only you can interact. Add nicknames above.",
                    );
                }

                println!();
                print_bullet("Optional authentication (press Enter to skip each):");

                let server_password: String = Input::new()
                    .with_prompt("  Server password (for bouncers like ZNC, leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let nickserv_password: String = Input::new()
                    .with_prompt("  NickServ password (leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let sasl_password: String = Input::new()
                    .with_prompt("  SASL PLAIN password (leave empty if none)")
                    .allow_empty(true)
                    .interact_text()?;

                let verify_tls: bool = Confirm::new()
                    .with_prompt("  Verify TLS certificate?")
                    .default(true)
                    .interact()?;

                println!(
                    "  {} IRC configured as {}@{}:{}",
                    style("✅").green().bold(),
                    style(&nickname).cyan(),
                    style(&server).cyan(),
                    style(port).cyan()
                );

                config.irc = Some(IrcConfig {
                    server: server.trim().to_string(),
                    port,
                    nickname: nickname.trim().to_string(),
                    username: None,
                    channels,
                    allowed_users,
                    server_password: if server_password.trim().is_empty() {
                        None
                    } else {
                        Some(server_password.trim().to_string())
                    },
                    nickserv_password: if nickserv_password.trim().is_empty() {
                        None
                    } else {
                        Some(nickserv_password.trim().to_string())
                    },
                    sasl_password: if sasl_password.trim().is_empty() {
                        None
                    } else {
                        Some(sasl_password.trim().to_string())
                    },
                    verify_tls: Some(verify_tls),
                });
            }
            7 => {
                // ── Webhook ──
                println!();
                println!(
                    "  {} {}",
                    style("Webhook Setup").white().bold(),
                    style("— HTTP endpoint for custom integrations").dim()
                );

                let port: String = Input::new()
                    .with_prompt("  Port")
                    .default("8080".into())
                    .interact_text()?;

                let secret: String = Input::new()
                    .with_prompt("  Secret (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;

                config.webhook = Some(WebhookConfig {
                    port: port.parse().unwrap_or(8080),
                    secret: if secret.is_empty() {
                        None
                    } else {
                        Some(secret)
                    },
                });
                println!(
                    "  {} Webhook on port {}",
                    style("✅").green().bold(),
                    style(&port).cyan()
                );
            }
            _ => break, // Done
        }
        println!();
    }

    // Summary line
    let mut active: Vec<&str> = vec!["CLI"];
    if config.telegram.is_some() {
        active.push("Telegram");
    }
    if config.discord.is_some() {
        active.push("Discord");
    }
    if config.slack.is_some() {
        active.push("Slack");
    }
    if config.imessage.is_some() {
        active.push("iMessage");
    }
    if config.matrix.is_some() {
        active.push("Matrix");
    }
    if config.whatsapp.is_some() {
        active.push("WhatsApp");
    }
    if config.email.is_some() {
        active.push("Email");
    }
    if config.irc.is_some() {
        active.push("IRC");
    }
    if config.webhook.is_some() {
        active.push("Webhook");
    }

    println!(
        "  {} Channels: {}",
        style("✓").green().bold(),
        style(active.join(", ")).green()
    );

    Ok(config)
}

// ── Step 4: Tunnel ──────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub fn setup_tunnel() -> Result<crate::config::TunnelConfig> {
    use crate::config::schema::{
        CloudflareTunnelConfig, CustomTunnelConfig, NgrokTunnelConfig, TailscaleTunnelConfig,
        TunnelConfig,
    };

    print_bullet("A tunnel exposes your gateway to the internet securely.");
    print_bullet("Skip this if you only use CLI or local channels.");
    println!();

    let options = vec![
        "Skip — local only (default)",
        "Cloudflare Tunnel — Zero Trust, free tier",
        "Tailscale — private tailnet or public Funnel",
        "ngrok — instant public URLs",
        "Custom — bring your own (bore, frp, ssh, etc.)",
    ];

    let choice = Select::new()
        .with_prompt("  Select tunnel provider")
        .items(&options)
        .default(0)
        .interact()?;

    let config = match choice {
        1 => {
            println!();
            print_bullet("Get your tunnel token from the Cloudflare Zero Trust dashboard.");
            let token: String = Input::new()
                .with_prompt("  Cloudflare tunnel token")
                .interact_text()?;
            if token.trim().is_empty() {
                println!("  {} Skipped", style("→").dim());
                TunnelConfig::default()
            } else {
                println!(
                    "  {} Tunnel: {}",
                    style("✓").green().bold(),
                    style("Cloudflare").green()
                );
                TunnelConfig {
                    provider: "cloudflare".into(),
                    cloudflare: Some(CloudflareTunnelConfig { token }),
                    ..TunnelConfig::default()
                }
            }
        }
        2 => {
            println!();
            print_bullet("Tailscale must be installed and authenticated (tailscale up).");
            let funnel = Confirm::new()
                .with_prompt("  Use Funnel (public internet)? No = tailnet only")
                .default(false)
                .interact()?;
            println!(
                "  {} Tunnel: {} ({})",
                style("✓").green().bold(),
                style("Tailscale").green(),
                if funnel {
                    "Funnel — public"
                } else {
                    "Serve — tailnet only"
                }
            );
            TunnelConfig {
                provider: "tailscale".into(),
                tailscale: Some(TailscaleTunnelConfig {
                    funnel,
                    hostname: None,
                }),
                ..TunnelConfig::default()
            }
        }
        3 => {
            println!();
            print_bullet(
                "Get your auth token at https://dashboard.ngrok.com/get-started/your-authtoken",
            );
            let auth_token: String = Input::new()
                .with_prompt("  ngrok auth token")
                .interact_text()?;
            if auth_token.trim().is_empty() {
                println!("  {} Skipped", style("→").dim());
                TunnelConfig::default()
            } else {
                let domain: String = Input::new()
                    .with_prompt("  Custom domain (optional, Enter to skip)")
                    .allow_empty(true)
                    .interact_text()?;
                println!(
                    "  {} Tunnel: {}",
                    style("✓").green().bold(),
                    style("ngrok").green()
                );
                TunnelConfig {
                    provider: "ngrok".into(),
                    ngrok: Some(NgrokTunnelConfig {
                        auth_token,
                        domain: if domain.is_empty() {
                            None
                        } else {
                            Some(domain)
                        },
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        4 => {
            println!();
            print_bullet("Enter the command to start your tunnel.");
            print_bullet("Use {port} and {host} as placeholders.");
            print_bullet("Example: bore local {port} --to bore.pub");
            let cmd: String = Input::new()
                .with_prompt("  Start command")
                .interact_text()?;
            if cmd.trim().is_empty() {
                println!("  {} Skipped", style("→").dim());
                TunnelConfig::default()
            } else {
                println!(
                    "  {} Tunnel: {} ({})",
                    style("✓").green().bold(),
                    style("Custom").green(),
                    style(&cmd).dim()
                );
                TunnelConfig {
                    provider: "custom".into(),
                    custom: Some(CustomTunnelConfig {
                        start_command: cmd,
                        health_url: None,
                        url_pattern: None,
                    }),
                    ..TunnelConfig::default()
                }
            }
        }
        _ => {
            println!(
                "  {} Tunnel: {}",
                style("✓").green().bold(),
                style("none (local only)").dim()
            );
            TunnelConfig::default()
        }
    };

    Ok(config)
}

// ── Step 6: Scaffold workspace files ─────────────────────────────

#[allow(clippy::too_many_lines)]
fn input_workspace_path() -> Result<String> {
    let custom: String = Input::new()
        .with_prompt("  Enter workspace path")
        .interact_text()?;
    let expanded = shellexpand::tilde(&custom).to_string();
    validate_non_empty("workspace path", &expanded)
}
