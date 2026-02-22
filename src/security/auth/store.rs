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
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub order: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub last_good: HashMap<String, String>,
    #[serde(default)]
    pub usage_stats: HashMap<String, ProfileUsageStats>,
    #[serde(default)]
    pub profiles: Vec<AuthProfile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileUsageStats {
    #[serde(default)]
    pub last_used_at: Option<i64>,
    #[serde(default)]
    pub cooldown_until: Option<i64>,
    #[serde(default)]
    pub error_count: u32,
}

impl Default for AuthProfileStore {
    fn default() -> Self {
        Self {
            version: AUTH_PROFILES_VERSION,
            defaults: HashMap::new(),
            order: HashMap::new(),
            last_good: HashMap::new(),
            usage_stats: HashMap::new(),
            profiles: Vec::new(),
        }
    }
}

impl AuthProfileStore {
    fn unix_now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .unwrap_or(0)
    }

    fn provider_profile_indexes(&self, provider: &str) -> Vec<usize> {
        let canonical = canonical_provider_name(provider);
        self.profiles
            .iter()
            .enumerate()
            .filter_map(|(index, profile)| {
                (!profile.disabled && canonical_provider_name(&profile.provider) == canonical)
                    .then_some(index)
            })
            .collect()
    }

    fn cooldown_active(stats: Option<&ProfileUsageStats>, now_ts: i64) -> bool {
        stats
            .and_then(|value| value.cooldown_until)
            .is_some_and(|until| until > now_ts)
    }

    fn pick_profile_index_for_provider(
        &self,
        provider: &str,
        ignore_cooldown: bool,
    ) -> Option<usize> {
        let canonical = canonical_provider_name(provider);
        let now_ts = Self::unix_now();
        let candidate_indexes = self.provider_profile_indexes(&canonical);
        if candidate_indexes.is_empty() {
            return None;
        }

        let is_candidate = |profile_id: &str| {
            candidate_indexes.iter().copied().find(|index| {
                let profile = &self.profiles[*index];
                if profile.id != profile_id {
                    return false;
                }
                if ignore_cooldown {
                    return true;
                }
                let stats = self.usage_stats.get(profile_id);
                !Self::cooldown_active(stats, now_ts)
            })
        };

        if let Some(default_id) = self.defaults.get(&canonical)
            && let Some(index) = is_candidate(default_id)
        {
            return Some(index);
        }

        if let Some(order_list) = self.order.get(&canonical)
            && let Some(index) = order_list
                .iter()
                .find_map(|profile_id| is_candidate(profile_id))
        {
            return Some(index);
        }

        if let Some(last_good_id) = self.last_good.get(&canonical)
            && let Some(index) = is_candidate(last_good_id)
        {
            return Some(index);
        }

        candidate_indexes
            .into_iter()
            .filter(|index| {
                if ignore_cooldown {
                    return true;
                }
                let profile_id = &self.profiles[*index].id;
                let stats = self.usage_stats.get(profile_id);
                !Self::cooldown_active(stats, now_ts)
            })
            .min_by_key(|index| {
                let profile_id = &self.profiles[*index].id;
                self.usage_stats
                    .get(profile_id)
                    .and_then(|stats| stats.last_used_at)
                    .unwrap_or(0)
            })
    }

    fn normalize_metadata(&mut self) -> bool {
        let mut changed = false;

        let mut provider_ids: HashMap<String, Vec<String>> = HashMap::new();
        for profile in &self.profiles {
            let canonical = canonical_provider_name(&profile.provider);
            provider_ids
                .entry(canonical)
                .or_default()
                .push(profile.id.clone());
        }

        self.defaults.retain(|provider, profile_id| {
            let keep = provider_ids
                .get(provider)
                .is_some_and(|ids| ids.iter().any(|id| id == profile_id));
            if !keep {
                changed = true;
            }
            keep
        });

        self.last_good.retain(|provider, profile_id| {
            let keep = provider_ids
                .get(provider)
                .is_some_and(|ids| ids.iter().any(|id| id == profile_id));
            if !keep {
                changed = true;
            }
            keep
        });

        self.usage_stats.retain(|profile_id, _| {
            let keep = self
                .profiles
                .iter()
                .any(|profile| profile.id == *profile_id);
            if !keep {
                changed = true;
            }
            keep
        });

        self.order.retain(|provider, ordered_ids| {
            let Some(provider_profile_ids) = provider_ids.get(provider) else {
                changed = true;
                return false;
            };

            let mut deduped = Vec::new();
            for profile_id in ordered_ids.iter() {
                if provider_profile_ids.iter().any(|id| id == profile_id)
                    && !deduped.iter().any(|id| id == profile_id)
                {
                    deduped.push(profile_id.clone());
                }
            }

            for profile_id in provider_profile_ids {
                if !deduped.iter().any(|id| id == profile_id) {
                    deduped.push(profile_id.clone());
                }
            }

            if *ordered_ids != deduped {
                *ordered_ids = deduped;
                changed = true;
            }

            true
        });

        for (provider, profile_ids) in &provider_ids {
            if !self.order.contains_key(provider) {
                self.order.insert(provider.clone(), profile_ids.clone());
                changed = true;
            }
        }

        changed
    }

    pub fn set_profile_order(&mut self, provider: &str, ordered_profile_ids: &[String]) {
        let canonical = canonical_provider_name(provider);
        let mut filtered = Vec::new();
        for profile_id in ordered_profile_ids {
            if self.profiles.iter().any(|profile| {
                canonical_provider_name(&profile.provider) == canonical && profile.id == *profile_id
            }) && !filtered.iter().any(|id| id == profile_id)
            {
                filtered.push(profile_id.clone());
            }
        }
        for profile in &self.profiles {
            if canonical_provider_name(&profile.provider) != canonical {
                continue;
            }
            if !filtered.iter().any(|id| id == &profile.id) {
                filtered.push(profile.id.clone());
            }
        }
        self.order.insert(canonical, filtered);
    }

    pub fn mark_profile_used(&mut self, provider: &str, profile_id: &str) {
        let canonical = canonical_provider_name(provider);
        self.last_good.insert(canonical, profile_id.to_string());
        let entry = self.usage_stats.entry(profile_id.to_string()).or_default();
        entry.last_used_at = Some(Self::unix_now());
        entry.cooldown_until = None;
        entry.error_count = 0;
    }

    pub fn mark_profile_failed(&mut self, profile_id: &str, cooldown_secs: Option<i64>) {
        let entry = self.usage_stats.entry(profile_id.to_string()).or_default();
        entry.error_count = entry.error_count.saturating_add(1);
        if let Some(seconds) = cooldown_secs.filter(|value| *value > 0) {
            entry.cooldown_until = Some(Self::unix_now().saturating_add(seconds));
        }
    }

    pub(super) fn active_profile_for_provider(&self, provider: &str) -> Option<&AuthProfile> {
        self.active_profile_index_for_provider(provider)
            .map(|index| &self.profiles[index])
    }

    pub(super) fn active_profile_index_for_provider(&self, provider: &str) -> Option<usize> {
        self.pick_profile_index_for_provider(provider, false)
            .or_else(|| self.pick_profile_index_for_provider(provider, true))
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
            match decrypt_secret_option(&mut profile.api_key, store, encrypt_enabled) {
                Ok(changed) => needs_persist |= changed,
                Err(e) => {
                    tracing::warn!(
                        profile_id = %profile.id,
                        provider = %profile.provider,
                        "Failed to decrypt api_key for auth profile — disabling: {e:#}"
                    );
                    profile.api_key = None;
                    profile.disabled = true;
                    needs_persist = true;
                }
            }
            match decrypt_secret_option(&mut profile.refresh_token, store, encrypt_enabled) {
                Ok(changed) => needs_persist |= changed,
                Err(e) => {
                    tracing::warn!(
                        profile_id = %profile.id,
                        provider = %profile.provider,
                        "Failed to decrypt refresh_token for auth profile — clearing: {e:#}"
                    );
                    profile.refresh_token = None;
                    needs_persist = true;
                }
            }
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

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).with_context(|| {
                format!(
                    "Failed to set auth profile store permissions on '{}': expected 0600",
                    path.display()
                )
            })?;
        }

        Ok(())
    }

    pub fn load_or_init_for_config(config: &Config) -> Result<Self> {
        let auth_profiles_path = auth_profiles_path(config);
        let store = auth_secret_store(config);

        let (mut profile_store, mut needs_persist) =
            Self::load_from_disk(&auth_profiles_path, &store, config.secrets.encrypt)?;

        needs_persist |= profile_store.normalize_metadata();

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

    pub(crate) fn upsert_profile(
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

        let profile_id_owned = profile_id.to_string();
        self.profiles.push(AuthProfile {
            id: profile_id_owned.clone(),
            provider: canonical_provider.clone(),
            label: normalized_label,
            api_key: normalized_api_key,
            refresh_token: normalized_refresh_token,
            auth_scheme: normalized_auth_scheme,
            oauth_source: normalized_oauth_source,
            disabled: false,
        });

        self.order
            .entry(canonical_provider.clone())
            .or_default()
            .push(profile_id_owned.clone());
        self.usage_stats
            .entry(profile_id_owned.clone())
            .or_default();

        if set_default {
            self.defaults
                .insert(canonical_provider.clone(), profile_id_owned.clone());
            self.last_good.insert(canonical_provider, profile_id_owned);
        }

        Ok(true)
    }
}
