use super::oauth::{
    OAuthProvider, claude_auth_status, codex_login_status, import_claude_oauth, import_codex_oauth,
    load_codex_auth_file,
};
use super::{
    AuthBroker, AuthProfile, AuthProfileStore, auth_profiles_path, canonical_provider_name,
    has_secret,
};
use crate::config::Config;
use anyhow::{Context, Result, bail};
use dialoguer::Password;
use std::io::IsTerminal;
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

#[allow(clippy::needless_pass_by_value)]
pub fn handle_command(command: crate::AuthCommands, config: &Config) -> Result<()> {
    match command {
        crate::AuthCommands::List => handle_list(config),
        crate::AuthCommands::Status { provider } => handle_status(config, provider.as_deref()),
        crate::AuthCommands::Login {
            provider,
            profile,
            label,
            api_key,
            no_default,
        } => handle_login(
            config,
            provider.as_str(),
            profile.as_deref(),
            label,
            api_key,
            no_default,
        ),
        crate::AuthCommands::OAuthLogin {
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
            label,
            no_default,
            skip_cli_login,
            setup_token,
        ),
        crate::AuthCommands::OAuthStatus { provider } => {
            handle_oauth_status(config, provider.as_deref())
        }
    }
}

fn handle_list(config: &Config) -> Result<()> {
    let store = AuthProfileStore::load_or_init_for_config(config)?;
    let path = auth_profiles_path(config);

    println!("🔐 Auth profiles");
    println!("Store: {}", path.display());
    println!(
        "Encryption: {}",
        if config.secrets.encrypt {
            "enabled"
        } else {
            "disabled"
        }
    );

    if store.profiles.is_empty() {
        println!();
        println!("No auth profiles yet.");
        println!(
            "Create one with: asteroniris auth login --provider {}",
            config.default_provider.as_deref().unwrap_or("openrouter")
        );
        return Ok(());
    }

    let mut profiles: Vec<&AuthProfile> = store.profiles.iter().collect();
    profiles.sort_by(|a, b| {
        canonical_provider_name(&a.provider)
            .cmp(&canonical_provider_name(&b.provider))
            .then_with(|| a.id.cmp(&b.id))
    });

    println!();
    let now_ts = unix_now();
    for profile in profiles {
        let provider = canonical_provider_name(&profile.provider);
        let is_default = store
            .defaults
            .get(&provider)
            .is_some_and(|default_id| default_id == &profile.id);
        let default_marker = if is_default { "*" } else { " " };
        let status = if profile.disabled {
            "disabled"
        } else {
            "active"
        };
        let key_state = if has_secret(profile.api_key.as_deref()) {
            "set"
        } else {
            "missing"
        };
        let label = profile
            .label
            .as_deref()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or("-");
        let usage = store.usage_stats.get(&profile.id);
        let cooldown_state = usage
            .and_then(|value| value.cooldown_until)
            .filter(|until| *until > now_ts)
            .map_or_else(
                || "ready".to_string(),
                |until| format!("cooldown-until-{until}"),
            );
        let last_used = usage
            .and_then(|value| value.last_used_at)
            .map_or_else(|| "-".to_string(), |value| value.to_string());
        let error_count = usage.map_or(0, |value| value.error_count);

        let auth_scheme = profile.auth_scheme.as_deref().unwrap_or("api_key");

        println!(
            "{default_marker} {} | provider={} | auth={} | status={} | key={} | label={} | cooldown={} | errors={} | last_used={}",
            profile.id,
            provider,
            auth_scheme,
            status,
            key_state,
            label,
            cooldown_state,
            error_count,
            last_used
        );
    }

    let stale_defaults: Vec<_> = store
        .defaults
        .iter()
        .filter(|(provider, profile_id)| {
            !store.profiles.iter().any(|profile| {
                canonical_provider_name(&profile.provider) == **provider
                    && profile.id == **profile_id
            })
        })
        .collect();

    if !stale_defaults.is_empty() {
        println!();
        println!("⚠️  Stale default mappings:");
        for (provider, profile_id) in stale_defaults {
            println!("  provider={provider} -> {profile_id} (missing profile)");
        }
    }

    println!();
    println!("* marks provider default profile");
    Ok(())
}

fn handle_status(config: &Config, provider: Option<&str>) -> Result<()> {
    let broker = AuthBroker::load_or_init(config)?;
    let store = AuthProfileStore::load_or_init_for_config(config)?;

    let requested_provider = provider
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");
    let canonical_provider = canonical_provider_name(requested_provider);

    let active_profile = store.active_profile_for_provider(&canonical_provider);
    let default_profile_id = store.defaults.get(&canonical_provider);
    let has_resolved_key = broker
        .resolve_provider_api_key(&canonical_provider)
        .is_some();
    let uses_config_key = active_profile.is_none() && has_secret(config.api_key.as_deref());

    println!("🔐 Auth status");
    println!("Provider: {canonical_provider}");
    println!(
        "Resolved key: {}",
        if has_resolved_key { "yes" } else { "no" }
    );

    match active_profile {
        Some(profile) => {
            println!("Source: profile");
            println!("Profile id: {}", profile.id);
            println!("Profile label: {}", profile.label.as_deref().unwrap_or("-"));
            println!(
                "Profile key: {}",
                if has_secret(profile.api_key.as_deref()) {
                    "set"
                } else {
                    "missing"
                }
            );
            println!(
                "Profile disabled: {}",
                if profile.disabled { "yes" } else { "no" }
            );
            println!(
                "Auth scheme: {}",
                profile.auth_scheme.as_deref().unwrap_or("api_key")
            );
            println!(
                "OAuth source: {}",
                profile.oauth_source.as_deref().unwrap_or("-")
            );
        }
        None if uses_config_key => {
            println!("Source: config.api_key");
            println!("Profile id: -");
        }
        None => {
            println!("Source: none");
            println!("Profile id: -");
        }
    }

    println!(
        "Default mapping: {}",
        default_profile_id.map_or("(none)", String::as_str)
    );
    println!(
        "Config api_key: {}",
        if has_secret(config.api_key.as_deref()) {
            "set"
        } else {
            "missing"
        }
    );

    let memory_key_resolved = broker.resolve_memory_api_key(&config.memory).is_some();
    println!();
    println!(
        "Memory embedding provider: {}",
        config.memory.embedding_provider
    );
    println!(
        "Memory embedding key resolved: {}",
        if memory_key_resolved { "yes" } else { "no" }
    );

    Ok(())
}

fn handle_login(
    config: &Config,
    provider: &str,
    profile: Option<&str>,
    label: Option<String>,
    api_key: Option<String>,
    no_default: bool,
) -> Result<()> {
    let canonical_provider = canonical_provider_name(provider);
    if canonical_provider.is_empty() {
        bail!("Provider cannot be empty");
    }

    let mut store = AuthProfileStore::load_or_init_for_config(config)?;
    let profile_id = profile
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map_or_else(
            || format!("{canonical_provider}-default"),
            ToOwned::to_owned,
        );

    let api_key_value = if let Some(key) = api_key {
        key
    } else {
        if !std::io::stdin().is_terminal() {
            bail!("--api-key is required in non-interactive mode");
        }
        Password::new()
            .with_prompt(format!(
                "API key for provider '{canonical_provider}' (input hidden)"
            ))
            .allow_empty_password(false)
            .interact()
            .context("Failed to read API key from terminal")?
    };

    let created = store.upsert_profile(
        AuthProfile {
            id: profile_id.clone(),
            provider: canonical_provider.clone(),
            label,
            api_key: Some(api_key_value),
            refresh_token: None,
            auth_scheme: Some("api_key".into()),
            oauth_source: None,
            disabled: false,
        },
        !no_default,
    )?;

    store.mark_profile_used(&canonical_provider, &profile_id);

    store.save_for_config(config)?;

    println!(
        "✅ {} auth profile '{}' for provider '{}'",
        if created { "Created" } else { "Updated" },
        profile_id,
        canonical_provider
    );
    println!(
        "Default mapping: {}",
        if no_default {
            "unchanged"
        } else {
            "set to this profile"
        }
    );
    println!(
        "Storage: {} ({})",
        auth_profiles_path(config).display(),
        if config.secrets.encrypt {
            "encrypted"
        } else {
            "plaintext"
        }
    );

    Ok(())
}

fn handle_oauth_login(
    config: &Config,
    provider: &str,
    profile: Option<&str>,
    label: Option<String>,
    no_default: bool,
    skip_cli_login: bool,
    setup_token: Option<String>,
) -> Result<()> {
    let oauth_provider = OAuthProvider::parse(provider)?;
    let mut store = AuthProfileStore::load_or_init_for_config(config)?;

    let imported = match oauth_provider {
        OAuthProvider::Codex => import_codex_oauth(skip_cli_login)?,
        OAuthProvider::Claude => import_claude_oauth(skip_cli_login, setup_token)?,
    };

    let profile_id = profile
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map_or_else(
            || imported.default_profile_id.to_string(),
            ToOwned::to_owned,
        );

    let final_label = label.or_else(|| Some(imported.default_label.to_string()));

    let created = store.upsert_profile(
        AuthProfile {
            id: profile_id.clone(),
            provider: imported.target_provider.to_string(),
            label: final_label,
            api_key: Some(imported.access_token),
            refresh_token: imported.refresh_token,
            auth_scheme: Some("oauth".into()),
            oauth_source: Some(imported.source_name.to_string()),
            disabled: false,
        },
        !no_default,
    )?;

    store.mark_profile_used(imported.target_provider, &profile_id);

    store.save_for_config(config)?;

    println!(
        "✅ {} OAuth profile '{}' for provider '{}'",
        if created { "Created" } else { "Updated" },
        profile_id,
        imported.target_provider
    );
    println!("OAuth source: {}", imported.source_name);
    println!(
        "Default mapping: {}",
        if no_default {
            "unchanged"
        } else {
            "set to this profile"
        }
    );
    println!(
        "Storage: {} ({})",
        auth_profiles_path(config).display(),
        if config.secrets.encrypt {
            "encrypted"
        } else {
            "plaintext"
        }
    );

    Ok(())
}

fn handle_oauth_status(config: &Config, provider: Option<&str>) -> Result<()> {
    let filter = provider.map(OAuthProvider::parse).transpose()?;
    let store = AuthProfileStore::load_or_init_for_config(config)?;

    println!("🔐 OAuth source status");

    if filter.is_none() || filter == Some(OAuthProvider::Codex) {
        println!();
        println!("[codex/openai]");

        match codex_login_status() {
            Ok(status) => println!("CLI status: {status}"),
            Err(err) => println!("CLI status: unavailable ({err})"),
        }

        match load_codex_auth_file() {
            Ok(parsed) => {
                let has_access = parsed
                    .tokens
                    .as_ref()
                    .and_then(|t| t.access_token.as_deref())
                    .is_some_and(|t| !t.trim().is_empty());
                let has_refresh = parsed
                    .tokens
                    .as_ref()
                    .and_then(|t| t.refresh_token.as_deref())
                    .is_some_and(|t| !t.trim().is_empty());
                println!(
                    "Local token cache: {}",
                    if has_access { "present" } else { "missing" }
                );
                println!(
                    "Refresh token cache: {}",
                    if has_refresh { "present" } else { "missing" }
                );
            }
            Err(err) => println!("Local token cache: unavailable ({err})"),
        }

        let has_profile = store.profiles.iter().any(|p| {
            canonical_provider_name(&p.provider) == "openai"
                && p.auth_scheme.as_deref() == Some("oauth")
                && !p.disabled
                && has_secret(p.api_key.as_deref())
        });
        println!(
            "Stored OAuth profile (openai): {}",
            if has_profile { "yes" } else { "no" }
        );
    }

    if filter.is_none() || filter == Some(OAuthProvider::Claude) {
        println!();
        println!("[claude/anthropic]");

        match claude_auth_status() {
            Ok(status) => {
                println!(
                    "CLI logged in: {}",
                    if status.logged_in { "yes" } else { "no" }
                );
                println!(
                    "CLI auth method: {}",
                    status.auth_method.as_deref().unwrap_or("unknown")
                );
            }
            Err(err) => println!("CLI status: unavailable ({err})"),
        }

        let has_profile = store.profiles.iter().any(|p| {
            canonical_provider_name(&p.provider) == "anthropic"
                && p.auth_scheme.as_deref() == Some("oauth")
                && !p.disabled
                && has_secret(p.api_key.as_deref())
        });

        println!(
            "Stored OAuth profile (anthropic): {}",
            if has_profile { "yes" } else { "no" }
        );
        println!(
            "Note: anthropic OAuth uses setup token (sk-ant-oat01-...). Import via `asteroniris auth oauth-login --provider claude`, set ANTHROPIC_OAUTH_TOKEN, or run `claude setup-token`."
        );
    }

    Ok(())
}
