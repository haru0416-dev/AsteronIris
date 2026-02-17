use crate::config::{Config, MemoryConfig};
use crate::security::SecretStore;
use anyhow::{bail, Context, Result};
use dialoguer::Password;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

const AUTH_PROFILES_FILENAME: &str = "auth-profiles.json";
const AUTH_PROFILES_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfileStore {
    #[serde(default = "default_auth_profiles_version")]
    pub version: u32,
    #[serde(default)]
    pub defaults: HashMap<String, String>,
    #[serde(default)]
    pub profiles: Vec<AuthProfile>,
}

fn default_auth_profiles_version() -> u32 {
    AUTH_PROFILES_VERSION
}

impl Default for AuthProfileStore {
    fn default() -> Self {
        Self {
            version: AUTH_PROFILES_VERSION,
            defaults: HashMap::new(),
            profiles: Vec::new(),
        }
    }
}

impl AuthProfileStore {
    fn active_profile_for_provider(&self, provider: &str) -> Option<&AuthProfile> {
        let canonical = canonical_provider_name(provider);

        if let Some(default_id) = self.defaults.get(&canonical) {
            let profile = self.profiles.iter().find(|p| {
                !p.disabled
                    && p.id == *default_id
                    && canonical_provider_name(&p.provider) == canonical
            });
            if profile.is_some() {
                return profile;
            }
        }

        self.profiles
            .iter()
            .find(|p| !p.disabled && canonical_provider_name(&p.provider) == canonical)
    }

    fn active_api_key_for_provider(&self, provider: &str) -> Option<String> {
        self.active_profile_for_provider(provider)
            .and_then(|profile| profile.api_key.as_deref())
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(ToOwned::to_owned)
    }

    fn migrate_legacy_config_api_key(&mut self, provider: &str, legacy_api_key: &str) -> bool {
        let legacy_api_key = legacy_api_key.trim();
        if legacy_api_key.is_empty() {
            return false;
        }

        let canonical = canonical_provider_name(provider);
        if self.active_profile_for_provider(&canonical).is_some() {
            return false;
        }

        let mut profile_id = format!("{canonical}-legacy-default");
        while self.profiles.iter().any(|p| p.id == profile_id) {
            profile_id.push('x');
        }

        self.profiles.push(AuthProfile {
            id: profile_id.clone(),
            provider: canonical.clone(),
            label: Some("Migrated from config.api_key".into()),
            api_key: Some(legacy_api_key.to_string()),
            disabled: false,
        });
        self.defaults.insert(canonical, profile_id);

        true
    }

    fn load_from_disk(
        path: &Path,
        store: &SecretStore,
        encrypt_enabled: bool,
    ) -> Result<(Self, bool)> {
        if !path.exists() {
            return Ok((Self::default(), false));
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read auth profile store: {}", path.display()))?;
        let mut loaded: Self = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse auth profile store: {}", path.display()))?;

        let mut needs_persist = false;

        for profile in &mut loaded.profiles {
            needs_persist |= decrypt_secret_option(&mut profile.api_key, store, encrypt_enabled)?;
        }

        Ok((loaded, needs_persist))
    }

    fn save_to_disk(&self, path: &Path, store: &SecretStore, encrypt_enabled: bool) -> Result<()> {
        let mut persisted = self.clone();

        if encrypt_enabled {
            for profile in &mut persisted.profiles {
                encrypt_secret_option(&mut profile.api_key, store)?;
            }
        }

        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create auth profile store parent directory: {}",
                parent.display()
            )
        })?;

        let json = serde_json::to_string_pretty(&persisted)?;
        fs::write(path, json)
            .with_context(|| format!("Failed to write auth profile store: {}", path.display()))?;

        Ok(())
    }

    pub fn load_or_init_for_config(config: &Config) -> Result<Self> {
        let auth_profiles_path = auth_profiles_path(config);
        let store = auth_secret_store(config);

        let (mut profile_store, mut needs_persist) =
            Self::load_from_disk(&auth_profiles_path, &store, config.secrets.encrypt)?;

        if let (Some(default_provider), Some(legacy_api_key)) = (
            config.default_provider.as_deref(),
            config.api_key.as_deref(),
        ) {
            if profile_store.migrate_legacy_config_api_key(default_provider, legacy_api_key) {
                needs_persist = true;
            }
        }

        if needs_persist {
            profile_store.save_to_disk(&auth_profiles_path, &store, config.secrets.encrypt)?;
        }

        Ok(profile_store)
    }

    pub fn save_for_config(&self, config: &Config) -> Result<()> {
        let auth_profiles_path = auth_profiles_path(config);
        let store = auth_secret_store(config);
        self.save_to_disk(&auth_profiles_path, &store, config.secrets.encrypt)
    }

    fn upsert_profile(&mut self, profile: AuthProfile, set_default: bool) -> Result<bool> {
        let profile_id = profile.id.trim();
        if profile_id.is_empty() {
            bail!("Profile id cannot be empty");
        }
        if !is_valid_profile_id(profile_id) {
            bail!("Invalid profile id '{profile_id}'. Use letters, numbers, '-', '_', or '.'");
        }

        let canonical_provider = canonical_provider_name(&profile.provider);
        if canonical_provider.is_empty() {
            bail!("Provider cannot be empty");
        }

        let normalized_label = profile.label.and_then(|label| {
            let trimmed = label.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

        let normalized_api_key = profile.api_key.and_then(|key| {
            let trimmed = key.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

        if let Some(existing) = self.profiles.iter_mut().find(|p| p.id == profile_id) {
            if canonical_provider_name(&existing.provider) != canonical_provider {
                bail!(
                    "Profile id '{profile_id}' already belongs to provider '{}'",
                    existing.provider
                );
            }

            existing.provider.clone_from(&canonical_provider);
            existing.label = normalized_label;
            existing.api_key = normalized_api_key;
            existing.disabled = false;

            if set_default {
                self.defaults
                    .insert(canonical_provider, profile_id.to_string());
            }
            return Ok(false);
        }

        self.profiles.push(AuthProfile {
            id: profile_id.to_string(),
            provider: canonical_provider.clone(),
            label: normalized_label,
            api_key: normalized_api_key,
            disabled: false,
        });

        if set_default {
            self.defaults
                .insert(canonical_provider, profile_id.to_string());
        }

        Ok(true)
    }
}

#[derive(Debug, Clone)]
pub struct AuthBroker {
    profile_store: AuthProfileStore,
    legacy_api_key: Option<String>,
}

impl AuthBroker {
    pub fn load_or_init(config: &Config) -> Result<Self> {
        let profile_store = AuthProfileStore::load_or_init_for_config(config)?;

        Ok(Self {
            profile_store,
            legacy_api_key: config
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(ToOwned::to_owned),
        })
    }

    pub fn resolve_provider_api_key(&self, provider: &str) -> Option<String> {
        self.profile_store
            .active_api_key_for_provider(provider)
            .or_else(|| self.legacy_api_key.clone())
    }

    pub fn resolve_memory_api_key(&self, memory: &MemoryConfig) -> Option<String> {
        let provider = memory.embedding_provider.trim();
        if provider.eq_ignore_ascii_case("openai") || provider.starts_with("custom:") {
            return self.resolve_provider_api_key("openai");
        }

        None
    }
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

        println!(
            "{default_marker} {} | provider={} | status={} | key={} | label={}",
            profile.id, provider, status, key_state, label
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
    let uses_legacy = active_profile.is_none() && has_secret(config.api_key.as_deref());

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
        }
        None if uses_legacy => {
            println!("Source: legacy config.api_key fallback");
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
        "Legacy config.api_key: {}",
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
            disabled: false,
        },
        !no_default,
    )?;

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

fn auth_secret_store(config: &Config) -> SecretStore {
    let secret_root = config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    SecretStore::new(secret_root, config.secrets.encrypt)
}

fn has_secret(secret: Option<&str>) -> bool {
    secret.map(str::trim).is_some_and(|value| !value.is_empty())
}

fn is_valid_profile_id(profile_id: &str) -> bool {
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

fn canonical_provider_name(name: &str) -> String {
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
        _ if normalized.starts_with("custom:") => "custom".to_string(),
        _ if normalized.starts_with("anthropic-custom:") => "anthropic-custom".to_string(),
        _ => normalized,
    }
}

fn decrypt_secret_option(
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
    let (decrypted, migrated) = store.decrypt_and_migrate(trimmed)?;
    *value = Some(decrypted);

    Ok(needs_encrypt_persist || migrated.is_some())
}

fn encrypt_secret_option(value: &mut Option<String>, store: &SecretStore) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(tmp: &tempfile::TempDir) -> Config {
        let mut config = Config::default();
        config.config_path = tmp.path().join("config.toml");
        config.workspace_dir = tmp.path().join("workspace");
        config
    }

    #[test]
    fn broker_migrates_legacy_config_api_key_to_profiles_store() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.default_provider = Some("openrouter".into());
        config.api_key = Some("sk-legacy-openrouter".into());
        config.secrets.encrypt = true;

        let broker = AuthBroker::load_or_init(&config).unwrap();
        assert_eq!(
            broker.resolve_provider_api_key("openrouter").as_deref(),
            Some("sk-legacy-openrouter")
        );

        let store_path = auth_profiles_path(&config);
        assert!(store_path.exists());

        let persisted = fs::read_to_string(store_path).unwrap();
        assert!(persisted.contains("enc2:"));
        assert!(!persisted.contains("sk-legacy-openrouter"));
    }

    #[test]
    fn broker_prefers_provider_profile_over_legacy_key() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.default_provider = Some("openrouter".into());
        config.api_key = Some("sk-legacy".into());
        config.secrets.encrypt = true;

        let path = auth_profiles_path(&config);
        fs::write(
            &path,
            r#"{
  "version": 1,
  "defaults": {
    "openrouter": "or-default",
    "openai": "oa-default"
  },
  "profiles": [
    {
      "id": "or-default",
      "provider": "openrouter",
      "api_key": "sk-openrouter-profile"
    },
    {
      "id": "oa-default",
      "provider": "openai",
      "api_key": "sk-openai-profile"
    }
  ]
}"#,
        )
        .unwrap();

        let broker = AuthBroker::load_or_init(&config).unwrap();
        assert_eq!(
            broker.resolve_provider_api_key("openrouter").as_deref(),
            Some("sk-openrouter-profile")
        );
        assert_eq!(
            broker.resolve_provider_api_key("openai").as_deref(),
            Some("sk-openai-profile")
        );
        assert_eq!(
            broker.resolve_provider_api_key("anthropic").as_deref(),
            Some("sk-legacy")
        );

        let persisted = fs::read_to_string(path).unwrap();
        assert!(persisted.contains("enc2:"));
        assert!(!persisted.contains("sk-openrouter-profile"));
        assert!(!persisted.contains("sk-openai-profile"));
    }

    #[test]
    fn broker_resolves_embedding_key_from_openai_profile() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.default_provider = Some("openrouter".into());
        config.api_key = Some("sk-legacy".into());
        config.memory.embedding_provider = "openai".into();

        let path = auth_profiles_path(&config);
        fs::write(
            &path,
            r#"{
  "version": 1,
  "defaults": {
    "openai": "oa-default"
  },
  "profiles": [
    {
      "id": "oa-default",
      "provider": "openai",
      "api_key": "sk-openai-profile"
    }
  ]
}"#,
        )
        .unwrap();

        let broker = AuthBroker::load_or_init(&config).unwrap();
        assert_eq!(
            broker.resolve_memory_api_key(&config.memory).as_deref(),
            Some("sk-openai-profile")
        );
    }

    #[test]
    fn upsert_profile_sets_provider_default_and_normalizes_values() {
        let mut store = AuthProfileStore::default();

        let created = store
            .upsert_profile(
                AuthProfile {
                    id: "openai-main".into(),
                    provider: "OpenAI".into(),
                    label: Some("  Primary Key  ".into()),
                    api_key: Some("  sk-openai-main  ".into()),
                    disabled: true,
                },
                true,
            )
            .unwrap();

        assert!(created);
        assert_eq!(store.profiles.len(), 1);
        assert_eq!(store.profiles[0].provider, "openai");
        assert_eq!(store.profiles[0].label.as_deref(), Some("Primary Key"));
        assert_eq!(store.profiles[0].api_key.as_deref(), Some("sk-openai-main"));
        assert!(!store.profiles[0].disabled);
        assert_eq!(
            store.defaults.get("openai"),
            Some(&"openai-main".to_string())
        );
    }

    #[test]
    fn upsert_profile_rejects_invalid_id() {
        let mut store = AuthProfileStore::default();
        let result = store.upsert_profile(
            AuthProfile {
                id: "bad id".into(),
                provider: "openrouter".into(),
                label: None,
                api_key: Some("sk-test".into()),
                disabled: false,
            },
            true,
        );

        assert!(result.is_err());
    }

    #[test]
    fn load_or_init_migrates_legacy_key_without_duplicate_profiles() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.default_provider = Some("openrouter".into());
        config.api_key = Some("sk-legacy-openrouter".into());
        config.secrets.encrypt = true;

        let store_first = AuthProfileStore::load_or_init_for_config(&config).unwrap();
        assert_eq!(store_first.profiles.len(), 1);

        let store_second = AuthProfileStore::load_or_init_for_config(&config).unwrap();
        assert_eq!(store_second.profiles.len(), 1);

        let persisted = fs::read_to_string(auth_profiles_path(&config)).unwrap();
        assert!(persisted.contains("enc2:"));
    }
}
