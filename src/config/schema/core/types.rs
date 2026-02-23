use super::super::{
    AutonomyConfig, ChannelsConfig, GatewayConfig, McpConfig, MemoryConfig, ObservabilityConfig,
    TasteConfig, ToolsConfig, TunnelConfig,
};
use crate::media::types::MediaConfig;
use anyhow::Result;
use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub media: MediaConfig,

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

    #[serde(default)]
    pub mcp: McpConfig,

    #[serde(default)]
    pub taste: TasteConfig,

    #[serde(default = "default_locale")]
    pub locale: String,
}

fn default_locale() -> String {
    "en".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    #[serde(default = "default_identity_format")]
    pub format: String,
    #[serde(default)]
    pub person_id: Option<String>,
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
            person_id: None,
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
            media: MediaConfig::default(),
            tunnel: TunnelConfig::default(),
            gateway: GatewayConfig::default(),
            composio: ComposioConfig::default(),
            secrets: SecretsConfig::default(),
            browser: BrowserConfig::default(),
            persona: PersonaConfig::default(),
            identity: IdentityConfig::default(),
            tools: ToolsConfig::default(),
            mcp: McpConfig::default(),
            taste: TasteConfig::default(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::core::test_env::{ENV_LOCK, EnvVarGuard};

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
        assert_eq!(deserialized.media.enabled, config.media.enabled);
        assert_eq!(deserialized.media.storage_dir, config.media.storage_dir);
        assert_eq!(
            deserialized.media.max_file_size_mb,
            config.media.max_file_size_mb
        );
        assert_eq!(deserialized.autonomy.level, config.autonomy.level);
        assert_eq!(
            deserialized.autonomy.external_action_execution,
            config.autonomy.external_action_execution
        );
        assert_eq!(deserialized.workspace_dir, PathBuf::new());
        assert_eq!(deserialized.config_path, PathBuf::new());
    }

    #[test]
    fn config_round_trip_with_default_media_config() {
        let config = Config::default();

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.media.enabled, MediaConfig::default().enabled);
        assert_eq!(
            deserialized.media.storage_dir,
            MediaConfig::default().storage_dir
        );
        assert_eq!(
            deserialized.media.max_file_size_mb,
            MediaConfig::default().max_file_size_mb
        );
    }

    #[test]
    fn config_round_trip_with_custom_media_config() {
        let config = Config {
            media: MediaConfig {
                enabled: true,
                storage_dir: Some("/tmp/custom-media".to_string()),
                max_file_size_mb: 64,
            },
            ..Config::default()
        };

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert!(deserialized.media.enabled);
        assert_eq!(
            deserialized.media.storage_dir,
            Some("/tmp/custom-media".to_string())
        );
        assert_eq!(deserialized.media.max_file_size_mb, 64);
    }
}
