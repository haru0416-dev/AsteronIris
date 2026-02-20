use anyhow::Result;

pub fn default_model_for_provider(provider: &str) -> String {
    match provider {
        "anthropic" => "claude-sonnet-4-6".into(),
        "openai" => "gpt-5.2".into(),
        "ollama" => "llama3.2".into(),
        "groq" => "llama-3.3-70b-versatile".into(),
        "deepseek" => "deepseek-chat".into(),
        "gemini" | "google" | "google-gemini" => "gemini-2.5-flash".into(),
        "mistral" => "mistral-large-2512".into(),
        "xai" | "grok" => "grok-4-0709".into(),
        "cohere" => "command-a-03-2025".into(),
        "moonshot" | "kimi" => "kimi-k2.5".into(),
        "glm" | "zhipu" => "glm-4.7".into(),
        "minimax" => "MiniMax-M2.1".into(),
        _ => "anthropic/claude-sonnet-4-6".into(),
    }
}

pub fn provider_env_var(name: &str) -> &'static str {
    match name {
        "openrouter" => "OPENROUTER_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "venice" => "VENICE_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "xai" | "grok" => "XAI_API_KEY",
        "together" | "together-ai" => "TOGETHER_API_KEY",
        "fireworks" | "fireworks-ai" => "FIREWORKS_API_KEY",
        "perplexity" => "PERPLEXITY_API_KEY",
        "cohere" => "COHERE_API_KEY",
        "moonshot" | "kimi" => "MOONSHOT_API_KEY",
        "glm" | "zhipu" => "GLM_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "qianfan" | "baidu" => "QIANFAN_API_KEY",
        "zai" | "z.ai" => "ZAI_API_KEY",
        "synthetic" => "SYNTHETIC_API_KEY",
        "opencode" | "opencode-zen" => "OPENCODE_API_KEY",
        "vercel" | "vercel-ai" => "VERCEL_API_KEY",
        "cloudflare" | "cloudflare-ai" => "CLOUDFLARE_API_KEY",
        "bedrock" | "aws-bedrock" => "AWS_ACCESS_KEY_ID",
        "gemini" | "google" | "google-gemini" => "GEMINI_API_KEY",
        _ => "API_KEY",
    }
}

// ── Step 5: Tool Mode & Security ────────────────────────────────

pub fn validate_non_empty(label: &str, value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{label} cannot be empty");
    }
    Ok(trimmed.to_string())
}

pub fn validate_base_url(value: &str) -> Result<String> {
    let normalized = validate_non_empty("base URL", value)?;
    Ok(normalized.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_env_var_known_providers() {
        assert_eq!(provider_env_var("openrouter"), "OPENROUTER_API_KEY");
        assert_eq!(provider_env_var("anthropic"), "ANTHROPIC_API_KEY");
        assert_eq!(provider_env_var("openai"), "OPENAI_API_KEY");
    }

    #[test]
    fn validate_non_empty_rejects_blank() {
        assert!(validate_non_empty("x", "   ").is_err());
    }

    #[test]
    fn validate_base_url_trims_trailing_slash() {
        assert_eq!(
            validate_base_url("https://ex.com/").unwrap(),
            "https://ex.com"
        );
    }

    #[test]
    fn validate_base_url_rejects_empty() {
        assert!(validate_base_url("   ").is_err());
    }

    #[test]
    fn validate_base_url_preserves_path() {
        assert_eq!(
            validate_base_url("https://ex.com/v1").unwrap(),
            "https://ex.com/v1"
        );
    }

    #[test]
    fn validate_base_url_trims_whitespace() {
        assert_eq!(
            validate_base_url("  https://ex.com  ").unwrap(),
            "https://ex.com"
        );
    }
}
