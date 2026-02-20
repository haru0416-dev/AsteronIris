use super::{
    AutonomyConfig, ChannelsConfig, GatewayConfig, MemoryConfig, ObservabilityConfig, ToolsConfig,
    TunnelConfig,
};
use crate::security::SecretStore;
use anyhow::{Context, Result};
use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Workspace directory - computed from home, not serialized
    #[serde(skip)]
    pub workspace_dir: PathBuf,
    /// Path to config.toml - computed from home, not serialized
    #[serde(skip)]
    pub config_path: PathBuf,
    pub api_key: Option<String>,
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub default_temperature: f64,

    #[serde(default)]
    pub observability: ObservabilityConfig,

    #[serde(default)]
    pub autonomy: AutonomyConfig,

    #[serde(default)]
    pub runtime: RuntimeConfig,

    #[serde(default)]
    pub reliability: ReliabilityConfig,

    #[serde(default)]
    pub heartbeat: HeartbeatConfig,

    #[serde(default)]
    pub channels_config: ChannelsConfig,

    #[serde(default)]
    pub memory: MemoryConfig,

    #[serde(default)]
    pub tunnel: TunnelConfig,

    #[serde(default)]
    pub gateway: GatewayConfig,

    #[serde(default)]
    pub composio: ComposioConfig,

    #[serde(default)]
    pub secrets: SecretsConfig,

    #[serde(default)]
    pub browser: BrowserConfig,

    #[serde(default)]
    pub persona: PersonaConfig,

    #[serde(default)]
    pub identity: IdentityConfig,

    #[serde(default)]
    pub tools: ToolsConfig,

    #[serde(default = "default_locale")]
    pub locale: String,
}

fn default_locale() -> String {
    "en".into()
}

/// Detect locale: `ASTERONIRIS_LANG` env → config value → system `LANG` → `"en"`.
fn detect_locale(config_locale: &str) -> String {
    if let Ok(lang) = std::env::var("ASTERONIRIS_LANG") {
        let lang = lang.trim().to_lowercase();
        if !lang.is_empty() {
            return normalise_locale(&lang);
        }
    }

    if config_locale != "en" && !config_locale.is_empty() {
        return normalise_locale(config_locale);
    }

    if let Ok(lang) = std::env::var("LANG").or_else(|_| std::env::var("LC_MESSAGES")) {
        let lang = lang.trim().to_lowercase();
        if !lang.is_empty() {
            return normalise_locale(&lang);
        }
    }

    "en".into()
}

/// Normalise `"ja_JP.UTF-8"` → `"ja"`, `"en_US"` → `"en"`, passthrough `"ja"`.
fn normalise_locale(raw: &str) -> String {
    let base = raw.split('.').next().unwrap_or(raw);
    let lang = base.split('_').next().unwrap_or(base);
    lang.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    #[serde(default = "default_identity_format")]
    pub format: String,
    #[serde(default)]
    pub aieos_path: Option<String>,
    #[serde(default)]
    pub aieos_inline: Option<String>,
}

fn default_identity_format() -> String {
    "markdown".into()
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            format: default_identity_format(),
            aieos_path: None,
            aieos_inline: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_entity_id")]
    pub entity_id: String,
}

fn default_entity_id() -> String {
    "default".into()
}

impl Default for ComposioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: None,
            entity_id: default_entity_id(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsConfig {
    #[serde(default = "default_true")]
    pub encrypt: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self { encrypt: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub session_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaConfig {
    #[serde(default)]
    pub enabled_main_session: bool,
    #[serde(default = "default_persona_state_mirror_file")]
    pub state_mirror_filename: String,
    #[serde(default = "default_persona_max_open_loops")]
    pub max_open_loops: usize,
    #[serde(default = "default_persona_max_next_actions")]
    pub max_next_actions: usize,
    #[serde(default = "default_persona_max_commitments")]
    pub max_commitments: usize,
    #[serde(default = "default_persona_max_current_objective_chars")]
    pub max_current_objective_chars: usize,
    #[serde(default = "default_persona_max_recent_context_summary_chars")]
    pub max_recent_context_summary_chars: usize,
    #[serde(default = "default_persona_max_list_item_chars")]
    pub max_list_item_chars: usize,
}

fn default_persona_state_mirror_file() -> String {
    "STATE.md".into()
}

fn default_persona_max_open_loops() -> usize {
    7
}

fn default_persona_max_next_actions() -> usize {
    3
}

fn default_persona_max_commitments() -> usize {
    5
}

fn default_persona_max_current_objective_chars() -> usize {
    280
}

fn default_persona_max_recent_context_summary_chars() -> usize {
    1_200
}

fn default_persona_max_list_item_chars() -> usize {
    240
}

impl Default for PersonaConfig {
    fn default() -> Self {
        Self {
            enabled_main_session: false,
            state_mirror_filename: default_persona_state_mirror_file(),
            max_open_loops: default_persona_max_open_loops(),
            max_next_actions: default_persona_max_next_actions(),
            max_commitments: default_persona_max_commitments(),
            max_current_objective_chars: default_persona_max_current_objective_chars(),
            max_recent_context_summary_chars: default_persona_max_recent_context_summary_chars(),
            max_list_item_chars: default_persona_max_list_item_chars(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum RuntimeKind {
    #[default]
    Native,
    Docker,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub kind: RuntimeKind,
    #[serde(default)]
    pub enable_docker_runtime: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityConfig {
    #[serde(default = "default_provider_retries")]
    pub provider_retries: u32,
    #[serde(default = "default_provider_backoff_ms")]
    pub provider_backoff_ms: u64,
    #[serde(default)]
    pub fallback_providers: Vec<String>,
    #[serde(default = "default_channel_backoff_secs")]
    pub channel_initial_backoff_secs: u64,
    #[serde(default = "default_channel_backoff_max_secs")]
    pub channel_max_backoff_secs: u64,
    #[serde(default = "default_scheduler_poll_secs")]
    pub scheduler_poll_secs: u64,
    #[serde(default = "default_scheduler_retries")]
    pub scheduler_retries: u32,
}

fn default_provider_retries() -> u32 {
    2
}

fn default_provider_backoff_ms() -> u64 {
    500
}

fn default_channel_backoff_secs() -> u64 {
    2
}

fn default_channel_backoff_max_secs() -> u64 {
    60
}

fn default_scheduler_poll_secs() -> u64 {
    15
}

fn default_scheduler_retries() -> u32 {
    2
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            provider_retries: default_provider_retries(),
            provider_backoff_ms: default_provider_backoff_ms(),
            fallback_providers: Vec::new(),
            channel_initial_backoff_secs: default_channel_backoff_secs(),
            channel_max_backoff_secs: default_channel_backoff_max_secs(),
            scheduler_poll_secs: default_scheduler_poll_secs(),
            scheduler_retries: default_scheduler_retries(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub interval_minutes: u32,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 30,
        }
    }
}

fn decrypt_secret_string(
    value: &mut String,
    store: &SecretStore,
    encrypt_enabled: bool,
) -> Result<bool> {
    let current = value.trim();
    if current.is_empty() {
        return Ok(false);
    }

    let needs_encrypt_persist = encrypt_enabled && !SecretStore::is_encrypted(current);
    let (decrypted, migrated) = store.decrypt_and_migrate(current)?;
    *value = decrypted;

    Ok(needs_encrypt_persist || migrated.is_some())
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

fn encrypt_secret_string(value: &mut String, store: &SecretStore) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || SecretStore::is_encrypted(trimmed) {
        if trimmed != value {
            *value = trimmed.to_string();
        }
        return Ok(());
    }

    *value = store.encrypt(trimmed)?;
    Ok(())
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

impl Default for Config {
    fn default() -> Self {
        let home =
            UserDirs::new().map_or_else(|| PathBuf::from("."), |u| u.home_dir().to_path_buf());
        let asteroniris_dir = home.join(".asteroniris");

        Self {
            workspace_dir: asteroniris_dir.join("workspace"),
            config_path: asteroniris_dir.join("config.toml"),
            api_key: None,
            default_provider: Some("openrouter".to_string()),
            default_model: Some("anthropic/claude-sonnet-4-20250514".to_string()),
            default_temperature: 0.7,
            observability: ObservabilityConfig::default(),
            autonomy: AutonomyConfig::default(),
            runtime: RuntimeConfig::default(),
            reliability: ReliabilityConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            channels_config: ChannelsConfig::default(),
            memory: MemoryConfig::default(),
            tunnel: TunnelConfig::default(),
            gateway: GatewayConfig::default(),
            composio: ComposioConfig::default(),
            secrets: SecretsConfig::default(),
            browser: BrowserConfig::default(),
            persona: PersonaConfig::default(),
            identity: IdentityConfig::default(),
            tools: ToolsConfig::default(),
            locale: default_locale(),
        }
    }
}

impl Config {
    pub fn validate_temperature_bands(&self) -> Result<()> {
        self.autonomy.validate_temperature_bands()
    }

    pub fn validate_autonomy_controls(&self) -> Result<()> {
        self.validate_temperature_bands()?;
        self.autonomy.validate_verify_repair_caps()?;
        Ok(())
    }

    fn secret_store_root(&self) -> &Path {
        self.config_path.parent().unwrap_or_else(|| Path::new("."))
    }

    fn secret_store(&self) -> SecretStore {
        SecretStore::new(self.secret_store_root(), self.secrets.encrypt)
    }

    fn decrypt_config_secrets_in_place(&mut self) -> Result<bool> {
        let store = self.secret_store();
        let mut needs_persist = false;

        needs_persist |= decrypt_secret_option(&mut self.api_key, &store, self.secrets.encrypt)?;
        needs_persist |=
            decrypt_secret_option(&mut self.composio.api_key, &store, self.secrets.encrypt)?;

        if let Some(telegram) = self.channels_config.telegram.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut telegram.bot_token, &store, self.secrets.encrypt)?;
        }

        if let Some(discord) = self.channels_config.discord.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut discord.bot_token, &store, self.secrets.encrypt)?;
        }

        if let Some(slack) = self.channels_config.slack.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut slack.bot_token, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut slack.app_token, &store, self.secrets.encrypt)?;
        }

        if let Some(webhook) = self.channels_config.webhook.as_mut() {
            needs_persist |=
                decrypt_secret_option(&mut webhook.secret, &store, self.secrets.encrypt)?;
        }

        if let Some(matrix) = self.channels_config.matrix.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut matrix.access_token, &store, self.secrets.encrypt)?;
        }

        if let Some(whatsapp) = self.channels_config.whatsapp.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut whatsapp.access_token, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_string(&mut whatsapp.verify_token, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut whatsapp.app_secret, &store, self.secrets.encrypt)?;
        }

        if let Some(irc) = self.channels_config.irc.as_mut() {
            needs_persist |=
                decrypt_secret_option(&mut irc.server_password, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut irc.nickserv_password, &store, self.secrets.encrypt)?;
            needs_persist |=
                decrypt_secret_option(&mut irc.sasl_password, &store, self.secrets.encrypt)?;
        }

        if let Some(cloudflare) = self.tunnel.cloudflare.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut cloudflare.token, &store, self.secrets.encrypt)?;
        }

        if let Some(ngrok) = self.tunnel.ngrok.as_mut() {
            needs_persist |=
                decrypt_secret_string(&mut ngrok.auth_token, &store, self.secrets.encrypt)?;
        }

        Ok(needs_persist)
    }

    fn encrypt_config_secrets_in_place(&mut self) -> Result<()> {
        if !self.secrets.encrypt {
            return Ok(());
        }

        let store = self.secret_store();

        encrypt_secret_option(&mut self.api_key, &store)?;
        encrypt_secret_option(&mut self.composio.api_key, &store)?;

        if let Some(telegram) = self.channels_config.telegram.as_mut() {
            encrypt_secret_string(&mut telegram.bot_token, &store)?;
        }

        if let Some(discord) = self.channels_config.discord.as_mut() {
            encrypt_secret_string(&mut discord.bot_token, &store)?;
        }

        if let Some(slack) = self.channels_config.slack.as_mut() {
            encrypt_secret_string(&mut slack.bot_token, &store)?;
            encrypt_secret_option(&mut slack.app_token, &store)?;
        }

        if let Some(webhook) = self.channels_config.webhook.as_mut() {
            encrypt_secret_option(&mut webhook.secret, &store)?;
        }

        if let Some(matrix) = self.channels_config.matrix.as_mut() {
            encrypt_secret_string(&mut matrix.access_token, &store)?;
        }

        if let Some(whatsapp) = self.channels_config.whatsapp.as_mut() {
            encrypt_secret_string(&mut whatsapp.access_token, &store)?;
            encrypt_secret_string(&mut whatsapp.verify_token, &store)?;
            encrypt_secret_option(&mut whatsapp.app_secret, &store)?;
        }

        if let Some(irc) = self.channels_config.irc.as_mut() {
            encrypt_secret_option(&mut irc.server_password, &store)?;
            encrypt_secret_option(&mut irc.nickserv_password, &store)?;
            encrypt_secret_option(&mut irc.sasl_password, &store)?;
        }

        if let Some(cloudflare) = self.tunnel.cloudflare.as_mut() {
            encrypt_secret_string(&mut cloudflare.token, &store)?;
        }

        if let Some(ngrok) = self.tunnel.ngrok.as_mut() {
            encrypt_secret_string(&mut ngrok.auth_token, &store)?;
        }

        Ok(())
    }

    fn config_for_persistence(&self) -> Result<Self> {
        let mut persisted = self.clone();
        persisted.encrypt_config_secrets_in_place()?;
        Ok(persisted)
    }

    pub fn load_or_init() -> Result<Self> {
        let home = UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .context("Could not find home directory")?;
        let asteroniris_dir = home.join(".asteroniris");
        let config_path = asteroniris_dir.join("config.toml");

        if !asteroniris_dir.exists() {
            fs::create_dir_all(&asteroniris_dir)
                .context("Failed to create .asteroniris directory")?;
            fs::create_dir_all(asteroniris_dir.join("workspace"))
                .context("Failed to create workspace directory")?;
        }

        if config_path.exists() {
            let contents =
                fs::read_to_string(&config_path).context("Failed to read config file")?;
            let mut config: Config =
                toml::from_str(&contents).context("Failed to parse config file")?;
            config.config_path.clone_from(&config_path);
            config.workspace_dir = asteroniris_dir.join("workspace");

            let secrets_need_persist = config.decrypt_config_secrets_in_place()?;
            if secrets_need_persist {
                config.save()?;
            }

            config.validate_autonomy_controls()?;
            Ok(config)
        } else {
            let config = Self {
                config_path: config_path.clone(),
                workspace_dir: asteroniris_dir.join("workspace"),
                ..Self::default()
            };
            config.validate_autonomy_controls()?;
            config.save()?;
            Ok(config)
        }
    }

    /// Detect locale from env → config → system, then set `rust_i18n::set_locale`.
    pub fn apply_locale(&self) {
        let locale = detect_locale(&self.locale);
        rust_i18n::set_locale(&locale);
    }

    pub fn apply_env_overrides(&mut self) {
        if let Ok(key) = std::env::var("ASTERONIRIS_API_KEY").or_else(|_| std::env::var("API_KEY"))
            && !key.is_empty()
        {
            self.api_key = Some(key);
        }

        if let Ok(provider) =
            std::env::var("ASTERONIRIS_PROVIDER").or_else(|_| std::env::var("PROVIDER"))
            && !provider.is_empty()
        {
            self.default_provider = Some(provider);
        }

        if let Ok(model) = std::env::var("ASTERONIRIS_MODEL")
            && !model.is_empty()
        {
            self.default_model = Some(model);
        }

        if let Ok(workspace) = std::env::var("ASTERONIRIS_WORKSPACE")
            && !workspace.is_empty()
        {
            self.workspace_dir = PathBuf::from(workspace);
        }

        if let Ok(port_str) =
            std::env::var("ASTERONIRIS_GATEWAY_PORT").or_else(|_| std::env::var("PORT"))
            && let Ok(port) = port_str.parse::<u16>()
        {
            self.gateway.port = port;
        }

        if let Ok(host) =
            std::env::var("ASTERONIRIS_GATEWAY_HOST").or_else(|_| std::env::var("HOST"))
            && !host.is_empty()
        {
            self.gateway.host = host;
        }

        if let Ok(temp_str) = std::env::var("ASTERONIRIS_TEMPERATURE")
            && let Ok(temp) = temp_str.parse::<f64>()
            && (0.0..=2.0).contains(&temp)
        {
            self.default_temperature = temp;
        }
    }

    /// Returns `true` when the config appears to be a fresh default that has
    /// never been through onboarding (no API key, no provider explicitly chosen,
    /// and no env var overrides).
    pub fn needs_onboarding(&self) -> bool {
        // If env var provides an API key, user has configured externally
        if std::env::var("ASTERONIRIS_API_KEY").is_ok() || std::env::var("API_KEY").is_ok() {
            return false;
        }
        self.api_key.is_none() && self.default_provider.is_none()
    }

    pub fn save(&self) -> Result<()> {
        let persisted = self.config_for_persistence()?;
        let toml_str = toml::to_string_pretty(&persisted).context("Failed to serialize config")?;
        fs::write(&self.config_path, toml_str).context("Failed to write config file")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                unsafe {
                    std::env::set_var(self.key, value);
                }
            } else {
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    #[test]
    fn detect_locale_uses_expected_priority_order() {
        let _lock = ENV_LOCK.lock().unwrap();

        let _lang = EnvVarGuard::set("LANG", "pt_BR.UTF-8");
        let _lc_messages = EnvVarGuard::set("LC_MESSAGES", "es_ES.UTF-8");

        let _asteroniris_lang = EnvVarGuard::set("ASTERONIRIS_LANG", "ja_JP.UTF-8");
        assert_eq!(detect_locale("fr_FR"), "ja");
        drop(_asteroniris_lang);

        assert_eq!(detect_locale("fr_FR"), "fr");
        assert_eq!(detect_locale("en"), "pt");

        let _lang_unset = EnvVarGuard::unset("LANG");
        assert_eq!(detect_locale("en"), "es");

        let _lc_messages_unset = EnvVarGuard::unset("LC_MESSAGES");
        assert_eq!(detect_locale("en"), "en");
    }

    #[test]
    fn normalise_locale_handles_common_formats() {
        assert_eq!(normalise_locale("ja_JP.UTF-8"), "ja");
        assert_eq!(normalise_locale("en_US"), "en");
        assert_eq!(normalise_locale("en"), "en");
        assert_eq!(normalise_locale(""), "");
    }

    #[test]
    fn needs_onboarding_is_true_without_api_key_or_provider() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _asteroniris_api_key = EnvVarGuard::unset("ASTERONIRIS_API_KEY");
        let _api_key = EnvVarGuard::unset("API_KEY");

        let config = Config {
            api_key: None,
            default_provider: None,
            ..Config::default()
        };

        assert!(config.needs_onboarding());
    }

    #[test]
    fn needs_onboarding_is_false_with_configured_api_key() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _asteroniris_api_key = EnvVarGuard::unset("ASTERONIRIS_API_KEY");
        let _api_key = EnvVarGuard::unset("API_KEY");

        let config = Config {
            api_key: Some("sk-configured".to_string()),
            default_provider: None,
            ..Config::default()
        };

        assert!(!config.needs_onboarding());
    }

    #[test]
    fn needs_onboarding_is_false_with_env_api_key() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _asteroniris_api_key = EnvVarGuard::set("ASTERONIRIS_API_KEY", "sk-env");
        let _api_key = EnvVarGuard::unset("API_KEY");

        let config = Config {
            api_key: None,
            default_provider: None,
            ..Config::default()
        };

        assert!(!config.needs_onboarding());
    }

    #[test]
    fn default_config_has_reasonable_values() {
        let config = Config::default();

        assert_eq!(config.api_key, None);
        assert!(config.default_provider.is_some());
        assert!(config.default_model.is_some());
        assert!((0.0..=2.0).contains(&config.default_temperature));
        assert!(config.workspace_dir.ends_with("workspace"));
        assert!(config.config_path.ends_with("config.toml"));
        assert_eq!(config.locale, "en");
    }

    #[test]
    fn config_toml_round_trip_preserves_serialized_fields() {
        let config = Config {
            api_key: Some("sk-test".into()),
            default_provider: Some("openrouter".into()),
            default_model: Some("anthropic/claude-sonnet-4-20250514".into()),
            default_temperature: 1.1,
            locale: "ja".into(),
            ..Config::default()
        };

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.api_key, config.api_key);
        assert_eq!(deserialized.default_provider, config.default_provider);
        assert_eq!(deserialized.default_model, config.default_model);
        assert_eq!(deserialized.default_temperature, config.default_temperature);
        assert_eq!(deserialized.locale, config.locale);
        assert_eq!(deserialized.autonomy.level, config.autonomy.level);
        assert_eq!(
            deserialized.autonomy.external_action_execution,
            config.autonomy.external_action_execution
        );
        assert_eq!(deserialized.workspace_dir, PathBuf::new());
        assert_eq!(deserialized.config_path, PathBuf::new());
    }
}
