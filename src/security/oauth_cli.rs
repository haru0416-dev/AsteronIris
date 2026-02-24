use crate::cli::commands::AuthCommands;
use crate::config::Config;
use anyhow::{Context, Result, bail};
use dialoguer::Password;
use std::io::IsTerminal;

pub fn handle_auth_command(command: AuthCommands, config: &Config) -> Result<()> {
    match command {
        AuthCommands::List => handle_list(config),
        AuthCommands::Status { provider } => handle_status(config, provider.as_deref()),
        AuthCommands::Login {
            provider,
            profile,
            label,
            api_key,
            no_default,
        } => handle_login(
            config,
            provider.as_str(),
            profile.as_deref(),
            label.as_deref(),
            api_key,
            no_default,
        ),
        AuthCommands::OAuthLogin {
            provider,
            profile,
            label,
            no_default,
            skip_cli_login,
            setup_token,
        } => handle_oauth_login(
            config,
            provider.as_str(),
            profile.as_deref(),
            label.as_deref(),
            no_default,
            skip_cli_login,
            setup_token,
        ),
        AuthCommands::OAuthStatus { provider } => handle_oauth_status(provider.as_deref()),
    }
}

fn handle_list(config: &Config) -> Result<()> {
    println!("Auth (v2 config-backed)");
    println!(
        "Default provider: {}",
        config.default_provider.as_deref().unwrap_or("(unset)")
    );
    println!(
        "Config API key: {}",
        if has_secret(config.api_key.as_deref()) {
            "set"
        } else {
            "missing"
        }
    );

    let openai_oauth = crate::security::oauth::import_oauth_access_token_for_provider("openai")
        .context("Failed to inspect OpenAI OAuth cache")?
        .is_some();
    let anthropic_oauth =
        crate::security::oauth::import_oauth_access_token_for_provider("anthropic")
            .context("Failed to inspect Anthropic OAuth token")?
            .is_some();

    println!(
        "OpenAI OAuth cache (~/.codex/auth.json): {}",
        if openai_oauth { "available" } else { "missing" }
    );
    println!(
        "Anthropic OAuth token env: {}",
        if anthropic_oauth {
            "available"
        } else {
            "missing"
        }
    );

    Ok(())
}

fn handle_status(config: &Config, provider: Option<&str>) -> Result<()> {
    let provider = if let Some(value) = provider {
        canonical_provider_or_bail(value)?
    } else {
        config
            .default_provider
            .clone()
            .unwrap_or_else(|| "openrouter".to_string())
    };

    let resolved = crate::llm::factory::resolve_api_key(&provider, config.api_key.as_deref());

    println!("Auth status");
    println!("Provider: {provider}");
    println!(
        "Resolved key: {}",
        if resolved.is_some() { "yes" } else { "no" }
    );
    println!(
        "Config API key: {}",
        if has_secret(config.api_key.as_deref()) {
            "set"
        } else {
            "missing"
        }
    );

    Ok(())
}

fn handle_login(
    config: &Config,
    provider: &str,
    profile: Option<&str>,
    label: Option<&str>,
    api_key: Option<String>,
    no_default: bool,
) -> Result<()> {
    if profile.is_some() || label.is_some() {
        println!("Note: profile/label are not persisted in v2 config-backed auth mode.");
    }

    let provider = canonical_provider_or_bail(provider)?;
    let api_key = if let Some(key) = api_key {
        let trimmed = key.trim().to_string();
        if trimmed.is_empty() {
            bail!("API key cannot be empty");
        }
        trimmed
    } else {
        prompt_api_key(&provider)?
    };

    let mut updated = config.clone();
    updated.api_key = Some(api_key);
    if !no_default {
        updated.default_provider = Some(provider.clone());
    }
    updated.save()?;

    println!("Saved API key to config.");
    println!(
        "Default provider: {}",
        updated.default_provider.as_deref().unwrap_or("(unchanged)")
    );
    println!("Config: {}", updated.config_path.display());

    Ok(())
}

fn handle_oauth_login(
    config: &Config,
    provider: &str,
    profile: Option<&str>,
    label: Option<&str>,
    no_default: bool,
    skip_cli_login: bool,
    setup_token: Option<String>,
) -> Result<()> {
    if provider.trim().is_empty() {
        bail!("--provider cannot be empty");
    }

    if profile.is_some() || label.is_some() {
        println!("Note: profile/label are not persisted in v2 config-backed auth mode.");
    }

    let imported = crate::security::oauth::import_oauth_access_token_for_provider_with_login(
        provider,
        skip_cli_login,
        setup_token,
    )?;

    let mut updated = config.clone();
    updated.api_key = Some(imported.access_token);
    if !no_default {
        updated.default_provider = Some(imported.target_provider.to_string());
        if imported.target_provider == "openai-codex" {
            updated.default_model = Some("gpt-5.3-codex".to_string());
        }
    }
    updated.save()?;

    println!("OAuth import succeeded (source: {}).", imported.source_name);
    println!(
        "Default provider: {}",
        updated.default_provider.as_deref().unwrap_or("(unchanged)")
    );
    println!("Config: {}", updated.config_path.display());

    Ok(())
}

fn handle_oauth_status(provider: Option<&str>) -> Result<()> {
    let provider = provider.map(normalize_oauth_status_provider).transpose()?;
    match provider.as_deref() {
        None => {
            print_openai_oauth_status()?;
            println!();
            print_anthropic_oauth_status()?;
            Ok(())
        }
        Some("openai" | "openai-codex" | "codex") => print_openai_oauth_status(),
        Some("anthropic" | "claude") => print_anthropic_oauth_status(),
        Some(value) => bail!(
            "Unsupported provider '{value}'. Use one of: openai, openai-codex, codex, anthropic, claude"
        ),
    }
}

fn print_openai_oauth_status() -> Result<()> {
    println!("OpenAI OAuth (Codex)");
    match crate::security::oauth::codex_login_status() {
        Ok(status) => println!("codex login status: {status}"),
        Err(err) => println!("codex login status: unavailable ({err})"),
    }

    let cached = crate::security::oauth::import_oauth_access_token_for_provider("openai")
        .context("Failed to inspect OpenAI OAuth cache")?
        .is_some();
    println!(
        "cached token in ~/.codex/auth.json: {}",
        if cached { "yes" } else { "no" }
    );
    Ok(())
}

fn print_anthropic_oauth_status() -> Result<()> {
    println!("Anthropic OAuth (Claude)");
    match crate::security::oauth::claude_auth_status() {
        Ok((logged_in, method)) => {
            println!("claude logged in: {}", if logged_in { "yes" } else { "no" });
            println!(
                "claude auth method: {}",
                method.unwrap_or_else(|| "unknown".to_string())
            );
        }
        Err(err) => println!("claude auth status: unavailable ({err})"),
    }

    let cached = crate::security::oauth::import_oauth_access_token_for_provider("anthropic")
        .context("Failed to inspect Anthropic OAuth token")?
        .is_some();
    println!(
        "setup token available via env: {}",
        if cached { "yes" } else { "no" }
    );
    Ok(())
}

fn prompt_api_key(provider: &str) -> Result<String> {
    if !std::io::stdin().is_terminal() {
        bail!("--api-key is required in non-interactive mode");
    }

    let key = Password::new()
        .with_prompt(format!("API key for {provider}"))
        .allow_empty_password(false)
        .interact()
        .context("Failed to read API key from terminal")?;

    let trimmed = key.trim().to_string();
    if trimmed.is_empty() {
        bail!("API key cannot be empty");
    }
    Ok(trimmed)
}

fn has_secret(secret: Option<&str>) -> bool {
    secret.map(str::trim).is_some_and(|value| !value.is_empty())
}

fn canonical_provider(provider: &str) -> String {
    let normalized = provider.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "codex" => "openai-codex".to_string(),
        "claude" => "anthropic".to_string(),
        _ => normalized,
    }
}

fn canonical_provider_or_bail(provider: &str) -> Result<String> {
    let normalized = canonical_provider(provider);
    if normalized.is_empty() {
        bail!("--provider cannot be empty");
    }
    Ok(normalized)
}

fn normalize_oauth_status_provider(provider: &str) -> Result<String> {
    let normalized = provider.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        bail!("--provider cannot be empty");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{canonical_provider, canonical_provider_or_bail, normalize_oauth_status_provider};

    #[test]
    fn canonical_provider_maps_aliases() {
        assert_eq!(canonical_provider("codex"), "openai-codex");
        assert_eq!(canonical_provider("claude"), "anthropic");
    }

    #[test]
    fn canonical_provider_or_bail_rejects_empty() {
        assert!(canonical_provider_or_bail("   ").is_err());
    }

    #[test]
    fn normalize_oauth_status_provider_rejects_empty() {
        assert!(normalize_oauth_status_provider("   ").is_err());
    }

    #[test]
    fn normalize_oauth_status_provider_trims_and_lowercases() {
        assert_eq!(
            normalize_oauth_status_provider("  CoDeX ").unwrap(),
            "codex"
        );
    }
}
