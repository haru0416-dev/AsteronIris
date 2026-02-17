use crate::config::{Config, MemoryConfig};
use crate::security::SecretStore;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
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
}

#[derive(Debug, Clone)]
pub struct AuthBroker {
    profile_store: AuthProfileStore,
    legacy_api_key: Option<String>,
}

impl AuthBroker {
    pub fn load_or_init(config: &Config) -> Result<Self> {
        let auth_profiles_path = auth_profiles_path(config);
        let secret_root = config
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let store = SecretStore::new(secret_root, config.secrets.encrypt);

        let (mut profile_store, mut needs_persist) =
            AuthProfileStore::load_from_disk(&auth_profiles_path, &store, config.secrets.encrypt)?;

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
}
