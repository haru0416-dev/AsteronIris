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
