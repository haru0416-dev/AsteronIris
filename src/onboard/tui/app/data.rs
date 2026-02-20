pub(super) fn provider_list_for_tier(tier: usize) -> Vec<String> {
    match tier {
        0 => vec![
            "OpenRouter — 200+ models, 1 API key (recommended)".into(),
            "Venice AI — privacy-first".into(),
            "Anthropic — Claude (direct)".into(),
            "OpenAI — GPT (direct)".into(),
            "DeepSeek — V3 & R1".into(),
            "Mistral — Large & Codestral".into(),
            "xAI — Grok".into(),
            "Perplexity — search-augmented".into(),
            "Google Gemini".into(),
        ],
        1 => vec![
            "Groq — ultra-fast LPU".into(),
            "Fireworks AI".into(),
            "Together AI".into(),
        ],
        2 => vec![
            "Vercel AI Gateway".into(),
            "Cloudflare AI Gateway".into(),
            "Amazon Bedrock".into(),
        ],
        3 => vec![
            "Moonshot — Kimi".into(),
            "GLM — ChatGLM / Zhipu".into(),
            "MiniMax".into(),
            "Qianfan — Baidu".into(),
            "Z.AI".into(),
            "Synthetic".into(),
            "OpenCode Zen".into(),
            "Cohere".into(),
        ],
        4 => vec!["Ollama — local models".into()],
        _ => vec![],
    }
}

pub(super) fn provider_id_for_selection(tier: usize, idx: usize) -> String {
    let ids: Vec<&str> = match tier {
        0 => vec![
            "openrouter",
            "venice",
            "anthropic",
            "openai",
            "deepseek",
            "mistral",
            "xai",
            "perplexity",
            "gemini",
        ],
        1 => vec!["groq", "fireworks", "together"],
        2 => vec!["vercel", "cloudflare", "bedrock"],
        3 => vec![
            "moonshot",
            "glm",
            "minimax",
            "qianfan",
            "zai",
            "synthetic",
            "opencode",
            "cohere",
        ],
        4 => vec!["ollama"],
        _ => vec![],
    };
    ids.get(idx).unwrap_or(&"openrouter").to_string()
}

pub(super) fn model_list_for_provider(provider: &str) -> Vec<String> {
    match provider {
        "openrouter" => vec![
            "Claude Sonnet 4.6 (balanced, recommended)".into(),
            "Claude Opus 4.6 (most capable)".into(),
            "GPT-5.2 (OpenAI flagship)".into(),
            "GPT-5 Mini (fast, cheap)".into(),
            "Gemini 2.5 Flash (Google, fast)".into(),
            "Llama 3.3 70B (open source)".into(),
            "DeepSeek V3.2 (affordable)".into(),
        ],
        "anthropic" => vec![
            "Claude Sonnet 4.6 (balanced, recommended)".into(),
            "Claude Opus 4.6 (most capable)".into(),
            "Claude Haiku 4.5 (fastest, cheapest)".into(),
        ],
        "openai" => vec![
            "GPT-5.2 (flagship)".into(),
            "GPT-5 Mini (fast, cheap)".into(),
            "GPT-4.1 (1M context, non-reasoning)".into(),
        ],
        "ollama" => vec![
            "Llama 3.2 (small, recommended local)".into(),
            "Llama 3.3 70B (best quality local)".into(),
            "Phi-4 14B (Microsoft, strong reasoning)".into(),
            "Qwen3 (multilingual, hybrid thinking)".into(),
        ],
        "gemini" => vec![
            "Gemini 2.5 Flash (fast, recommended)".into(),
            "Gemini 2.5 Pro (best quality)".into(),
            "Gemini 2.5 Flash Lite (cheapest)".into(),
        ],
        _ => vec!["Default model".into()],
    }
}

pub(super) fn model_id_for_selection(provider: &str, idx: usize) -> String {
    let ids: Vec<&str> = match provider {
        "openrouter" => vec![
            "anthropic/claude-sonnet-4-6",
            "anthropic/claude-opus-4-6",
            "openai/gpt-5.2",
            "openai/gpt-5-mini",
            "google/gemini-2.5-flash",
            "meta-llama/llama-3.3-70b-instruct",
            "deepseek/deepseek-chat",
        ],
        "anthropic" => vec!["claude-sonnet-4-6", "claude-opus-4-6", "claude-haiku-4-5"],
        "openai" => vec!["gpt-5.2", "gpt-5-mini", "gpt-4.1"],
        "ollama" => vec!["llama3.2", "llama3.3", "phi4", "qwen3"],
        "gemini" => vec![
            "gemini-2.5-flash",
            "gemini-2.5-pro",
            "gemini-2.5-flash-lite",
        ],
        _ => vec!["default"],
    };
    ids.get(idx).unwrap_or(&"default").to_string()
}
