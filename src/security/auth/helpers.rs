use crate::config::Config;
use crate::security::SecretStore;
use anyhow::Result;
use std::path::{Path, PathBuf};

use super::AUTH_PROFILES_FILENAME;

pub(super) fn auth_secret_store(config: &Config) -> SecretStore {
    let secret_root = config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    SecretStore::new(secret_root, config.secrets.encrypt)
}

pub(crate) fn has_secret(secret: Option<&str>) -> bool {
    secret.map(str::trim).is_some_and(|value| !value.is_empty())
}

pub(super) fn is_valid_profile_id(profile_id: &str) -> bool {
    profile_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn auth_profiles_path(config: &Config) -> PathBuf {
    config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(AUTH_PROFILES_FILENAME)
}

pub(crate) fn canonical_provider_name(name: &str) -> String {
    let normalized = name.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "google" | "google-gemini" => "gemini".to_string(),
        "xai" | "grok" => "xai".to_string(),
        "vercel" | "vercel-ai" => "vercel".to_string(),
        "cloudflare" | "cloudflare-ai" => "cloudflare".to_string(),
        "moonshot" | "kimi" => "moonshot".to_string(),
        "zai" | "z.ai" => "zai".to_string(),
        "glm" | "zhipu" => "glm".to_string(),
        "qianfan" | "baidu" => "qianfan".to_string(),
        "together" | "together-ai" => "together".to_string(),
        "fireworks" | "fireworks-ai" => "fireworks".to_string(),
        "opencode" | "opencode-zen" => "opencode".to_string(),
        "copilot" | "github-copilot" => "copilot".to_string(),
        "openai-codex" => "openai".to_string(),
        _ if normalized.starts_with("custom:") => "custom".to_string(),
        _ if normalized.starts_with("anthropic-custom:") => "anthropic-custom".to_string(),
        _ => normalized,
    }
}

pub(super) fn decrypt_secret_option(
    value: &mut Option<String>,
    store: &SecretStore,
    encrypt_enabled: bool,
) -> Result<bool> {
    let Some(current) = value.as_deref() else {
        return Ok(false);
    };

    let trimmed = current.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }

    let needs_encrypt_persist = encrypt_enabled && !SecretStore::is_encrypted(trimmed);
    let decrypted = store.decrypt(trimmed)?;
    *value = Some(decrypted);

    Ok(needs_encrypt_persist)
}

pub(super) fn encrypt_secret_option(value: &mut Option<String>, store: &SecretStore) -> Result<()> {
    let Some(current) = value.as_deref() else {
        return Ok(());
    };

    let trimmed = current.trim();
    if trimmed.is_empty() || SecretStore::is_encrypted(trimmed) {
        if trimmed != current {
            *value = Some(trimmed.to_string());
        }
        return Ok(());
    }

    *value = Some(store.encrypt(trimmed)?);
    Ok(())
}
