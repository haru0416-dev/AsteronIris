mod broker;
mod cli;
mod helpers;
mod oauth;
mod store;

#[cfg(test)]
mod tests;

pub use broker::{AuthBroker, recover_oauth_profile_for_provider};
pub use cli::handle_command;
pub use helpers::auth_profiles_path;
pub use store::AuthProfileStore;

pub(super) use helpers::{canonical_provider_name, has_secret};

use serde::{Deserialize, Serialize};

use anyhow::Result;
use oauth::{import_claude_oauth_cached, import_codex_oauth_cached};

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
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub auth_scheme: Option<String>,
    #[serde(default)]
    pub oauth_source: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

pub fn import_oauth_access_token_for_provider(provider: &str) -> Result<Option<(String, String)>> {
    let normalized = provider.trim().to_ascii_lowercase();
    let imported = match normalized.as_str() {
        "openai" => import_codex_oauth_cached(),
        "anthropic" => import_claude_oauth_cached()?,
        _ => None,
    };

    Ok(imported.map(|cred| (cred.access_token, cred.source_name.to_string())))
}
