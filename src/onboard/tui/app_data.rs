pub(super) fn provider_list_for_tier(tier: usize) -> Vec<String> {
    match tier {
        0 => vec![
            "OpenRouter — 200+ models, 1 API key (recommended)".into(),
            "Venice AI — privacy-first (Llama, Opus)".into(),
            "Anthropic — Claude Sonnet & Opus (direct)".into(),
            "OpenAI — GPT-5.3 Codex (OAuth), GPT-4o, o1, GPT-5".into(),
            "DeepSeek — V3 & R1 (affordable)".into(),
            "Mistral — Large & Codestral".into(),
            "xAI — Grok 3 & 4".into(),
            "Perplexity — search-augmented AI".into(),
            "Google Gemini — Gemini 2.0 Flash & Pro (supports CLI auth)".into(),
        ],
        1 => vec![
            "Groq — ultra-fast LPU inference".into(),
            "Fireworks AI — fast open-source inference".into(),
            "Together AI — open-source model hosting".into(),
        ],
        2 => vec![
            "Vercel AI Gateway".into(),
            "Cloudflare AI Gateway".into(),
            "Amazon Bedrock — AWS managed models".into(),
            "GitHub Copilot — OAuth-style developer models".into(),
        ],
        3 => vec![
            "Moonshot — Kimi & Kimi Coding".into(),
            "GLM — ChatGLM / Zhipu".into(),
            "MiniMax — MiniMax AI models".into(),
            "Qianfan — Baidu AI models".into(),
            "Z.AI — Z.AI inference".into(),
            "Synthetic — Synthetic AI models".into(),
            "OpenCode Zen — code-focused AI".into(),
            "Cohere — Command R+ & embeddings".into(),
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
        2 => vec!["vercel", "cloudflare", "bedrock", "copilot"],
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
            "GPT-5.3 Codex (OAuth profile, OpenAI Codex)".into(),
            "GPT-5.2 (flagship)".into(),
            "GPT-5 Mini (fast, cheap)".into(),
            "GPT-4.1 (1M context, non-reasoning)".into(),
        ],
        "venice" => vec![
            "DeepSeek V3.2 (recommended)".into(),
            "Claude Opus 4.6 via Venice (most capable)".into(),
            "Llama 3.3 70B (open source, fast)".into(),
        ],
        "groq" => vec![
            "Llama 3.3 70B (fast, recommended)".into(),
            "Llama 3.1 8B (instant)".into(),
            "GPT-OSS 120B (OpenAI open-weight)".into(),
        ],
        "mistral" => vec![
            "Mistral Large 3 (flagship)".into(),
            "Codestral (code-focused)".into(),
            "Mistral Small 3.2 (fast, cheap)".into(),
        ],
        "deepseek" => vec![
            "DeepSeek Chat (V3.2, recommended)".into(),
            "DeepSeek Reasoner (V3.2 thinking)".into(),
        ],
        "xai" => vec![
            "Grok 4 (flagship)".into(),
            "Grok 3 Mini (fast, cheap)".into(),
        ],
        "perplexity" => vec![
            "Sonar Pro (search + reasoning)".into(),
            "Sonar (search, fast)".into(),
            "Sonar Deep Research (expert research)".into(),
        ],
        "fireworks" => vec![
            "DeepSeek V3.2 (recommended)".into(),
            "Llama 3.3 70B".into(),
            "Qwen3 235B (code-optimized)".into(),
        ],
        "together" => vec![
            "Llama 3.3 70B Turbo (recommended)".into(),
            "Llama 3.1 8B Turbo (fast)".into(),
            "DeepSeek V3.1".into(),
        ],
        "cohere" => vec!["Command A (flagship)".into(), "Command R 7B (fast)".into()],
        "moonshot" => vec![
            "Kimi K2.5 (flagship, multimodal)".into(),
            "Kimi K2 Turbo (fast)".into(),
        ],
        "glm" => vec!["GLM-4.7 (flagship)".into(), "GLM-4.7 Flash (fast)".into()],
        "minimax" => vec!["MiniMax M2.1 (flagship)".into(), "MiniMax M2".into()],
        "copilot" => vec![
            "GPT-4.1 via Copilot (recommended)".into(),
            "GPT-4o via Copilot".into(),
            "Claude Sonnet via Copilot".into(),
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
        "openai" => vec!["gpt-5.3-codex", "gpt-5.2", "gpt-5-mini", "gpt-4.1"],
        "venice" => vec!["deepseek-v3.2", "claude-opus-4-6", "llama-3.3-70b"],
        "groq" => vec![
            "llama-3.3-70b-versatile",
            "llama-3.1-8b-instant",
            "openai/gpt-oss-120b",
        ],
        "mistral" => vec!["mistral-large-2512", "codestral-2508", "mistral-small-2506"],
        "deepseek" => vec!["deepseek-chat", "deepseek-reasoner"],
        "xai" => vec!["grok-4-0709", "grok-3-mini"],
        "perplexity" => vec!["sonar-pro", "sonar", "sonar-deep-research"],
        "fireworks" => vec![
            "accounts/fireworks/models/deepseek-v3p2",
            "accounts/fireworks/models/llama-v3p3-70b-instruct",
            "accounts/fireworks/models/qwen3-235b-a22b",
        ],
        "together" => vec![
            "meta-llama/Llama-3.3-70B-Instruct-Turbo",
            "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo",
            "deepseek-ai/DeepSeek-V3.1",
        ],
        "cohere" => vec!["command-a-03-2025", "command-r7b-12-2024"],
        "moonshot" => vec!["kimi-k2.5", "kimi-k2-turbo-preview"],
        "glm" => vec!["glm-4.7", "glm-4.7-flash"],
        "minimax" => vec!["MiniMax-M2.1", "MiniMax-M2"],
        "copilot" => vec!["gpt-4.1", "gpt-4o", "claude-3.7-sonnet"],
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
