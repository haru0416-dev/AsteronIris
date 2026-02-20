use super::helpers::{
    auth_profiles_path, auth_secret_store, canonical_provider_name, decrypt_secret_option,
    encrypt_secret_option, is_valid_profile_id,
};
use super::{AUTH_PROFILES_VERSION, AuthProfile};
use crate::config::Config;
use crate::security::SecretStore;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn default_auth_profiles_version() -> u32 {
    AUTH_PROFILES_VERSION
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
    pub(super) fn active_profile_for_provider(&self, provider: &str) -> Option<&AuthProfile> {
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

    pub(super) fn active_profile_index_for_provider(&self, provider: &str) -> Option<usize> {
        let canonical = canonical_provider_name(provider);

        if let Some(default_id) = self.defaults.get(&canonical) {
            let index = self.profiles.iter().position(|p| {
                !p.disabled
                    && p.id == *default_id
                    && canonical_provider_name(&p.provider) == canonical
            });
            if index.is_some() {
                return index;
            }
        }

        self.profiles
            .iter()
            .position(|p| !p.disabled && canonical_provider_name(&p.provider) == canonical)
    }

    pub(super) fn active_api_key_for_provider(&self, provider: &str) -> Option<String> {
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
            refresh_token: None,
            auth_scheme: Some("api_key".into()),
            oauth_source: None,
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
            needs_persist |=
                decrypt_secret_option(&mut profile.refresh_token, store, encrypt_enabled)?;
        }

        Ok((loaded, needs_persist))
    }

    fn save_to_disk(&self, path: &Path, store: &SecretStore, encrypt_enabled: bool) -> Result<()> {
        let mut persisted = self.clone();

        if encrypt_enabled {
            for profile in &mut persisted.profiles {
                encrypt_secret_option(&mut profile.api_key, store)?;
                encrypt_secret_option(&mut profile.refresh_token, store)?;
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
        ) && profile_store.migrate_legacy_config_api_key(default_provider, legacy_api_key)
        {
            needs_persist = true;
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

    pub(super) fn upsert_profile(
        &mut self,
        profile: AuthProfile,
        set_default: bool,
    ) -> Result<bool> {
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
        let normalized_refresh_token = profile.refresh_token.and_then(|key| {
            let trimmed = key.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        let normalized_auth_scheme = profile.auth_scheme.and_then(|kind| {
            let trimmed = kind.trim().to_ascii_lowercase();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        let normalized_oauth_source = profile.oauth_source.and_then(|source| {
            let trimmed = source.trim().to_ascii_lowercase();
            (!trimmed.is_empty()).then_some(trimmed)
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
            existing.refresh_token = normalized_refresh_token;
            existing.auth_scheme = normalized_auth_scheme;
            existing.oauth_source = normalized_oauth_source;
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
            refresh_token: normalized_refresh_token,
            auth_scheme: normalized_auth_scheme,
            oauth_source: normalized_oauth_source,
            disabled: false,
        });

        if set_default {
            self.defaults
                .insert(canonical_provider, profile_id.to_string());
        }

        Ok(true)
    }
}
