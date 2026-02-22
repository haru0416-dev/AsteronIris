use super::helpers::canonical_provider_name;
use super::oauth::{import_claude_oauth, import_codex_oauth};
use super::store::AuthProfileStore;
use crate::config::{Config, MemoryConfig};
use anyhow::Result;

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

pub fn recover_oauth_profile_for_provider(config: &Config, provider: &str) -> Result<bool> {
    let canonical = canonical_provider_name(provider);
    if canonical.is_empty() {
        return Ok(false);
    }

    let mut store = AuthProfileStore::load_or_init_for_config(config)?;
    let Some(index) = store.active_profile_index_for_provider(&canonical) else {
        return Ok(false);
    };

    let profile = &store.profiles[index];
    if profile.disabled || profile.auth_scheme.as_deref() != Some("oauth") {
        return Ok(false);
    }

    let oauth_source = profile
        .oauth_source
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let imported = match oauth_source.as_str() {
        "codex" => import_codex_oauth(true)?,
        "claude" => import_claude_oauth(true, None)?,
        _ => return Ok(false),
    };

    if canonical_provider_name(imported.target_provider) != canonical {
        return Ok(false);
    }

    let mut changed = false;
    {
        let profile = &mut store.profiles[index];

        if profile.api_key.as_deref() != Some(imported.access_token.as_str()) {
            profile.api_key = Some(imported.access_token.clone());
            changed = true;
        }

        if let Some(refresh_token) = imported.refresh_token
            && profile.refresh_token.as_deref() != Some(refresh_token.as_str())
        {
            profile.refresh_token = Some(refresh_token);
            changed = true;
        }

        if profile.auth_scheme.as_deref() != Some("oauth") {
            profile.auth_scheme = Some("oauth".into());
            changed = true;
        }

        if profile.oauth_source.as_deref() != Some(imported.source_name) {
            profile.oauth_source = Some(imported.source_name.into());
            changed = true;
        }
    }

    let profile_id = store.profiles[index].id.clone();
    store.mark_profile_used(&canonical, &profile_id);
    store.save_for_config(config)?;

    Ok(changed)
}
