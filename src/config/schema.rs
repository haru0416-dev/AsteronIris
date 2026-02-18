use crate::security::{AutonomyLevel, ExternalActionExecution, SecretStore};
use anyhow::{Context, Result};
use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ── Top-level config ──────────────────────────────────────────────

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
}

// ── Identity (AIEOS / Markdown format) ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    /// Identity format: "markdown" (default) or "aieos"
    #[serde(default = "default_identity_format")]
    pub format: String,
    /// Path to AIEOS JSON file (relative to workspace)
    #[serde(default)]
    pub aieos_path: Option<String>,
    /// Inline AIEOS JSON (alternative to file path)
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

// ── Gateway security ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GatewayDefenseMode {
    Audit,
    Warn,
    #[default]
    Enforce,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    /// Gateway port (default: 8080)
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    /// Gateway host (default: 127.0.0.1)
    #[serde(default = "default_gateway_host")]
    pub host: String,
    /// Require pairing before accepting requests (default: true)
    #[serde(default = "default_true")]
    pub require_pairing: bool,
    /// Allow binding to non-localhost without a tunnel (default: false)
    #[serde(default)]
    pub allow_public_bind: bool,
    /// Paired bearer tokens (managed automatically, not user-edited)
    #[serde(default)]
    pub paired_tokens: Vec<String>,
    #[serde(default)]
    pub defense_mode: GatewayDefenseMode,
    #[serde(default)]
    pub defense_kill_switch: bool,
}

fn default_gateway_port() -> u16 {
    3000
}

fn default_gateway_host() -> String {
    "127.0.0.1".into()
}

fn default_true() -> bool {
    true
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: default_gateway_port(),
            host: default_gateway_host(),
            require_pairing: true,
            allow_public_bind: false,
            paired_tokens: Vec::new(),
            defense_mode: GatewayDefenseMode::default(),
            defense_kill_switch: false,
        }
    }
}

// ── Composio (managed tool surface) ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioConfig {
    /// Enable Composio integration for 1000+ OAuth tools
    #[serde(default)]
    pub enabled: bool,
    /// Composio API key (stored encrypted when secrets.encrypt = true)
    #[serde(default)]
    pub api_key: Option<String>,
    /// Default entity ID for multi-user setups
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

// ── Secrets (encrypted credential store) ────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsConfig {
    /// Enable encryption for API keys and tokens in config.toml
    #[serde(default = "default_true")]
    pub encrypt: bool,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self { encrypt: true }
    }
}

// ── Browser (friendly-service browsing only) ───────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserConfig {
    /// Enable `browser_open` tool (opens URLs in Brave without scraping)
    #[serde(default)]
    pub enabled: bool,
    /// Allowed domains for `browser_open` (exact or subdomain match)
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    /// Browser session name (for agent-browser automation)
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

// ── Memory ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// "sqlite" | "lancedb" | "markdown" | "none"
    pub backend: String,
    /// Auto-save conversation context to memory
    pub auto_save: bool,
    /// Run memory/session hygiene (archiving + retention cleanup)
    #[serde(default = "default_hygiene_enabled")]
    pub hygiene_enabled: bool,
    /// Archive daily/session files older than this many days
    #[serde(default = "default_archive_after_days")]
    pub archive_after_days: u32,
    /// Purge archived files older than this many days
    #[serde(default = "default_purge_after_days")]
    pub purge_after_days: u32,
    /// For sqlite backend: prune conversation rows older than this many days
    #[serde(default = "default_conversation_retention_days")]
    pub conversation_retention_days: u32,
    #[serde(default)]
    pub layer_retention_working_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_episodic_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_semantic_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_procedural_days: Option<u32>,
    #[serde(default)]
    pub layer_retention_identity_days: Option<u32>,
    #[serde(default)]
    pub ledger_retention_days: Option<u32>,
    /// Embedding provider: "none" | "openai" | "custom:URL"
    #[serde(default = "default_embedding_provider")]
    pub embedding_provider: String,
    /// Embedding model name (e.g. "text-embedding-3-small")
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
    /// Embedding vector dimensions
    #[serde(default = "default_embedding_dims")]
    pub embedding_dimensions: usize,
    /// Weight for vector similarity in hybrid search (0.0–1.0)
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f64,
    /// Weight for keyword BM25 in hybrid search (0.0–1.0)
    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f64,
    /// Max embedding cache entries before LRU eviction
    #[serde(default = "default_cache_size")]
    pub embedding_cache_size: usize,
    /// Max tokens per chunk for document splitting
    #[serde(default = "default_chunk_size")]
    pub chunk_max_tokens: usize,
}

fn default_embedding_provider() -> String {
    "none".into()
}
fn default_hygiene_enabled() -> bool {
    true
}
fn default_archive_after_days() -> u32 {
    7
}
fn default_purge_after_days() -> u32 {
    30
}
fn default_conversation_retention_days() -> u32 {
    30
}
fn default_embedding_model() -> String {
    "text-embedding-3-small".into()
}
fn default_embedding_dims() -> usize {
    1536
}
fn default_vector_weight() -> f64 {
    0.7
}
fn default_keyword_weight() -> f64 {
    0.3
}
fn default_cache_size() -> usize {
    10_000
}
fn default_chunk_size() -> usize {
    512
}

impl MemoryConfig {
    pub fn layer_retention_days(&self, layer: &str) -> u32 {
        match layer {
            "working" => self
                .layer_retention_working_days
                .unwrap_or(self.conversation_retention_days),
            "episodic" => self
                .layer_retention_episodic_days
                .unwrap_or(self.conversation_retention_days),
            "semantic" => self
                .layer_retention_semantic_days
                .unwrap_or(self.conversation_retention_days),
            "procedural" => self
                .layer_retention_procedural_days
                .unwrap_or(self.conversation_retention_days),
            "identity" => self
                .layer_retention_identity_days
                .unwrap_or(self.conversation_retention_days),
            _ => self.conversation_retention_days,
        }
    }

    pub fn ledger_retention_or_default(&self) -> u32 {
        self.ledger_retention_days
            .unwrap_or(self.conversation_retention_days)
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".into(),
            auto_save: true,
            hygiene_enabled: default_hygiene_enabled(),
            archive_after_days: default_archive_after_days(),
            purge_after_days: default_purge_after_days(),
            conversation_retention_days: default_conversation_retention_days(),
            layer_retention_working_days: None,
            layer_retention_episodic_days: None,
            layer_retention_semantic_days: None,
            layer_retention_procedural_days: None,
            layer_retention_identity_days: None,
            ledger_retention_days: None,
            embedding_provider: default_embedding_provider(),
            embedding_model: default_embedding_model(),
            embedding_dimensions: default_embedding_dims(),
            vector_weight: default_vector_weight(),
            keyword_weight: default_keyword_weight(),
            embedding_cache_size: default_cache_size(),
            chunk_max_tokens: default_chunk_size(),
        }
    }
}

// ── Observability ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// "none" | "log" | "prometheus" | "otel"
    pub backend: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            backend: "none".into(),
        }
    }
}

// ── Autonomy / Security ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyConfig {
    pub level: AutonomyLevel,
    #[serde(default)]
    pub external_action_execution: ExternalActionExecution,
    #[serde(default)]
    pub rollout: AutonomyRolloutConfig,
    pub workspace_only: bool,
    pub allowed_commands: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub max_actions_per_hour: u32,
    pub max_cost_per_day_cents: u32,

    #[serde(default = "default_verify_repair_max_attempts")]
    pub verify_repair_max_attempts: u32,
    #[serde(default = "default_verify_repair_max_repair_depth")]
    pub verify_repair_max_repair_depth: u32,

    #[serde(default)]
    pub temperature_bands: TemperatureBandsConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AutonomyRolloutStage {
    #[default]
    Off,
    AuditOnly,
    Sanitize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyRolloutConfig {
    #[serde(default)]
    pub stage: AutonomyRolloutStage,
    #[serde(default)]
    pub verify_repair_enabled: bool,
    #[serde(default)]
    pub contradiction_weighting_enabled: bool,
    #[serde(default)]
    pub intent_audit_anomaly_detection_enabled: bool,
}

impl Default for AutonomyRolloutConfig {
    fn default() -> Self {
        Self {
            stage: AutonomyRolloutStage::Off,
            verify_repair_enabled: false,
            contradiction_weighting_enabled: false,
            intent_audit_anomaly_detection_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureBandsConfig {
    #[serde(default = "default_temperature_band_read_only")]
    pub read_only: TemperatureBand,
    #[serde(default = "default_temperature_band_supervised")]
    pub supervised: TemperatureBand,
    #[serde(default = "default_temperature_band_full")]
    pub full: TemperatureBand,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TemperatureBand {
    pub min: f64,
    pub max: f64,
}

fn default_temperature_band_read_only() -> TemperatureBand {
    TemperatureBand { min: 0.0, max: 0.6 }
}

fn default_temperature_band_supervised() -> TemperatureBand {
    TemperatureBand { min: 0.2, max: 1.0 }
}

fn default_temperature_band_full() -> TemperatureBand {
    TemperatureBand { min: 0.2, max: 1.2 }
}

fn default_verify_repair_max_attempts() -> u32 {
    3
}

fn default_verify_repair_max_repair_depth() -> u32 {
    2
}

impl Default for TemperatureBandsConfig {
    fn default() -> Self {
        Self {
            read_only: default_temperature_band_read_only(),
            supervised: default_temperature_band_supervised(),
            full: default_temperature_band_full(),
        }
    }
}

impl TemperatureBand {
    fn validate(self, label: &str) -> Result<()> {
        if self.min.is_nan() || self.max.is_nan() {
            anyhow::bail!("autonomy.temperature_bands.{label} min/max must not be NaN");
        }
        if !(0.0..=2.0).contains(&self.min) {
            anyhow::bail!("autonomy.temperature_bands.{label} min must be in [0.0, 2.0]");
        }
        if !(0.0..=2.0).contains(&self.max) {
            anyhow::bail!("autonomy.temperature_bands.{label} max must be in [0.0, 2.0]");
        }
        if self.min > self.max {
            anyhow::bail!("autonomy.temperature_bands.{label} min must be <= max");
        }
        Ok(())
    }
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            level: AutonomyLevel::Supervised,
            external_action_execution: ExternalActionExecution::Disabled,
            rollout: AutonomyRolloutConfig::default(),
            workspace_only: true,
            allowed_commands: vec![
                "git".into(),
                "npm".into(),
                "cargo".into(),
                "ls".into(),
                "cat".into(),
                "grep".into(),
                "find".into(),
                "echo".into(),
                "pwd".into(),
                "wc".into(),
                "head".into(),
                "tail".into(),
            ],
            forbidden_paths: vec![
                "/etc".into(),
                "/root".into(),
                "/home".into(),
                "/usr".into(),
                "/bin".into(),
                "/sbin".into(),
                "/lib".into(),
                "/opt".into(),
                "/boot".into(),
                "/dev".into(),
                "/proc".into(),
                "/sys".into(),
                "/var".into(),
                "/tmp".into(),
                "~/.ssh".into(),
                "~/.gnupg".into(),
                "~/.aws".into(),
                "~/.config".into(),
            ],
            max_actions_per_hour: 20,
            max_cost_per_day_cents: 500,
            verify_repair_max_attempts: default_verify_repair_max_attempts(),
            verify_repair_max_repair_depth: default_verify_repair_max_repair_depth(),
            temperature_bands: TemperatureBandsConfig::default(),
        }
    }
}

impl AutonomyConfig {
    pub fn selected_temperature_band(&self) -> TemperatureBand {
        match self.level {
            AutonomyLevel::ReadOnly => self.temperature_bands.read_only,
            AutonomyLevel::Supervised => self.temperature_bands.supervised,
            AutonomyLevel::Full => self.temperature_bands.full,
        }
    }

    pub fn clamp_temperature(&self, temperature: f64) -> f64 {
        let band = self.selected_temperature_band();
        temperature.clamp(band.min, band.max)
    }

    pub fn validate_temperature_bands(&self) -> Result<()> {
        self.temperature_bands.read_only.validate("read_only")?;
        self.temperature_bands.supervised.validate("supervised")?;
        self.temperature_bands.full.validate("full")?;
        Ok(())
    }

    pub fn validate_verify_repair_caps(&self) -> Result<()> {
        if self.verify_repair_max_attempts == 0 {
            anyhow::bail!("autonomy.verify_repair_max_attempts must be >= 1");
        }
        if self.verify_repair_max_repair_depth >= self.verify_repair_max_attempts {
            anyhow::bail!(
                "autonomy.verify_repair_max_repair_depth must be < autonomy.verify_repair_max_attempts"
            );
        }
        Ok(())
    }
}

// ── Runtime ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Runtime kind (currently supported: "native", "docker").
    ///
    /// Reserved value for this phase (unsupported): "cloudflare".
    pub kind: String,
    #[serde(default)]
    pub enable_docker_runtime: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            kind: "native".into(),
            enable_docker_runtime: false,
        }
    }
}

// ── Reliability / supervision ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityConfig {
    /// Retries per provider before failing over.
    #[serde(default = "default_provider_retries")]
    pub provider_retries: u32,
    /// Base backoff (ms) for provider retry delay.
    #[serde(default = "default_provider_backoff_ms")]
    pub provider_backoff_ms: u64,
    /// Fallback provider chain (e.g. `["anthropic", "openai"]`).
    #[serde(default)]
    pub fallback_providers: Vec<String>,
    /// Initial backoff for channel/daemon restarts.
    #[serde(default = "default_channel_backoff_secs")]
    pub channel_initial_backoff_secs: u64,
    /// Max backoff for channel/daemon restarts.
    #[serde(default = "default_channel_backoff_max_secs")]
    pub channel_max_backoff_secs: u64,
    /// Scheduler polling cadence in seconds.
    #[serde(default = "default_scheduler_poll_secs")]
    pub scheduler_poll_secs: u64,
    /// Max retries for cron job execution attempts.
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

// ── Heartbeat ────────────────────────────────────────────────────

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

// ── Tunnel ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    /// "none", "cloudflare", "tailscale", "ngrok", "custom"
    pub provider: String,

    #[serde(default)]
    pub cloudflare: Option<CloudflareTunnelConfig>,

    #[serde(default)]
    pub tailscale: Option<TailscaleTunnelConfig>,

    #[serde(default)]
    pub ngrok: Option<NgrokTunnelConfig>,

    #[serde(default)]
    pub custom: Option<CustomTunnelConfig>,
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            provider: "none".into(),
            cloudflare: None,
            tailscale: None,
            ngrok: None,
            custom: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareTunnelConfig {
    /// Cloudflare Tunnel token (from Zero Trust dashboard)
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleTunnelConfig {
    /// Use Tailscale Funnel (public internet) vs Serve (tailnet only)
    #[serde(default)]
    pub funnel: bool,
    /// Optional hostname override
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NgrokTunnelConfig {
    /// ngrok auth token
    pub auth_token: String,
    /// Optional custom domain
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTunnelConfig {
    /// Command template to start the tunnel. Use {port} and {host} placeholders.
    /// Example: "bore local {port} --to bore.pub"
    pub start_command: String,
    /// Optional URL to check tunnel health
    pub health_url: Option<String>,
    /// Optional regex to extract public URL from command stdout
    pub url_pattern: Option<String>,
}

// ── Channels ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub cli: bool,
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub webhook: Option<WebhookConfig>,
    pub imessage: Option<IMessageConfig>,
    pub matrix: Option<MatrixConfig>,
    pub whatsapp: Option<WhatsAppConfig>,
    pub email: Option<crate::channels::email_channel::EmailConfig>,
    pub irc: Option<IrcConfig>,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            cli: true,
            telegram: None,
            discord: None,
            slack: None,
            webhook: None,
            imessage: None,
            matrix: None,
            whatsapp: None,
            email: None,
            irc: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfig {
    pub bot_token: String,
    pub guild_id: Option<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: Option<String>,
    pub channel_id: Option<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub port: u16,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMessageConfig {
    pub allowed_contacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub access_token: String,
    pub room_id: String,
    pub allowed_users: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    /// Access token from Meta Business Suite
    pub access_token: String,
    /// Phone number ID from Meta Business API
    pub phone_number_id: String,
    /// Webhook verify token (you define this, Meta sends it back for verification)
    pub verify_token: String,
    /// App secret for webhook signature verification (X-Hub-Signature-256)
    #[serde(default)]
    pub app_secret: Option<String>,
    /// Allowed phone numbers (E.164 format: +1234567890) or "*" for all
    #[serde(default)]
    pub allowed_numbers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrcConfig {
    /// IRC server hostname
    pub server: String,
    /// IRC server port (default: 6697 for TLS)
    #[serde(default = "default_irc_port")]
    pub port: u16,
    /// Bot nickname
    pub nickname: String,
    /// Username (defaults to nickname if not set)
    pub username: Option<String>,
    /// Channels to join on connect
    #[serde(default)]
    pub channels: Vec<String>,
    /// Allowed nicknames (case-insensitive) or "*" for all
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// Server password (for bouncers like ZNC)
    pub server_password: Option<String>,
    /// `NickServ` IDENTIFY password
    pub nickserv_password: Option<String>,
    /// SASL PLAIN password (`IRCv3`)
    pub sasl_password: Option<String>,
    /// Verify TLS certificate (default: true)
    pub verify_tls: Option<bool>,
}

fn default_irc_port() -> u16 {
    6697
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

// ── Config impl ──────────────────────────────────────────────────

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
            // Set computed paths that are skipped during serialization
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

    /// Apply environment variable overrides to config
    pub fn apply_env_overrides(&mut self) {
        // API Key: ASTERONIRIS_API_KEY or API_KEY
        if let Ok(key) = std::env::var("ASTERONIRIS_API_KEY").or_else(|_| std::env::var("API_KEY"))
        {
            if !key.is_empty() {
                self.api_key = Some(key);
            }
        }

        // Provider: ASTERONIRIS_PROVIDER or PROVIDER
        if let Ok(provider) =
            std::env::var("ASTERONIRIS_PROVIDER").or_else(|_| std::env::var("PROVIDER"))
        {
            if !provider.is_empty() {
                self.default_provider = Some(provider);
            }
        }

        // Model: ASTERONIRIS_MODEL
        if let Ok(model) = std::env::var("ASTERONIRIS_MODEL") {
            if !model.is_empty() {
                self.default_model = Some(model);
            }
        }

        // Workspace directory: ASTERONIRIS_WORKSPACE
        if let Ok(workspace) = std::env::var("ASTERONIRIS_WORKSPACE") {
            if !workspace.is_empty() {
                self.workspace_dir = PathBuf::from(workspace);
            }
        }

        // Gateway port: ASTERONIRIS_GATEWAY_PORT or PORT
        if let Ok(port_str) =
            std::env::var("ASTERONIRIS_GATEWAY_PORT").or_else(|_| std::env::var("PORT"))
        {
            if let Ok(port) = port_str.parse::<u16>() {
                self.gateway.port = port;
            }
        }

        // Gateway host: ASTERONIRIS_GATEWAY_HOST or HOST
        if let Ok(host) =
            std::env::var("ASTERONIRIS_GATEWAY_HOST").or_else(|_| std::env::var("HOST"))
        {
            if !host.is_empty() {
                self.gateway.host = host;
            }
        }

        // Temperature: ASTERONIRIS_TEMPERATURE
        if let Ok(temp_str) = std::env::var("ASTERONIRIS_TEMPERATURE") {
            if let Ok(temp) = temp_str.parse::<f64>() {
                if (0.0..=2.0).contains(&temp) {
                    self.default_temperature = temp;
                }
            }
        }
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
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    // ── Defaults ─────────────────────────────────────────────

    #[test]
    fn config_default_has_sane_values() {
        let c = Config::default();
        assert_eq!(c.default_provider.as_deref(), Some("openrouter"));
        assert!(c.default_model.as_deref().unwrap().contains("claude"));
        assert!((c.default_temperature - 0.7).abs() < f64::EPSILON);
        assert!(c.api_key.is_none());
        assert!(c.workspace_dir.to_string_lossy().contains("workspace"));
        assert!(c.config_path.to_string_lossy().contains("config.toml"));
    }

    #[test]
    fn observability_config_default() {
        let o = ObservabilityConfig::default();
        assert_eq!(o.backend, "none");
    }

    #[test]
    fn autonomy_config_default() {
        let a = AutonomyConfig::default();
        assert_eq!(a.level, AutonomyLevel::Supervised);
        assert_eq!(a.rollout.stage, AutonomyRolloutStage::Off);
        assert!(!a.rollout.verify_repair_enabled);
        assert!(!a.rollout.contradiction_weighting_enabled);
        assert!(!a.rollout.intent_audit_anomaly_detection_enabled);
        assert!(a.workspace_only);
        assert!(a.allowed_commands.contains(&"git".to_string()));
        assert!(a.allowed_commands.contains(&"cargo".to_string()));
        assert!(a.forbidden_paths.contains(&"/etc".to_string()));
        assert_eq!(a.max_actions_per_hour, 20);
        assert_eq!(a.max_cost_per_day_cents, 500);
        assert_eq!(a.verify_repair_max_attempts, 3);
        assert_eq!(a.verify_repair_max_repair_depth, 2);
    }

    #[test]
    fn runtime_config_default() {
        let r = RuntimeConfig::default();
        assert_eq!(r.kind, "native");
    }

    #[test]
    fn heartbeat_config_default() {
        let h = HeartbeatConfig::default();
        assert!(!h.enabled);
        assert_eq!(h.interval_minutes, 30);
    }

    #[test]
    fn memory_config_default_hygiene_settings() {
        let m = MemoryConfig::default();
        assert_eq!(m.backend, "sqlite");
        assert!(m.auto_save);
        assert!(m.hygiene_enabled);
        assert_eq!(m.archive_after_days, 7);
        assert_eq!(m.purge_after_days, 30);
        assert_eq!(m.conversation_retention_days, 30);
        assert!(m.layer_retention_working_days.is_none());
        assert!(m.layer_retention_episodic_days.is_none());
        assert!(m.layer_retention_semantic_days.is_none());
        assert!(m.layer_retention_procedural_days.is_none());
        assert!(m.layer_retention_identity_days.is_none());
        assert!(m.ledger_retention_days.is_none());
    }

    #[test]
    fn memory_layer_retention_helpers() {
        let mut m = MemoryConfig::default();
        m.conversation_retention_days = 30;
        m.layer_retention_working_days = Some(7);
        m.layer_retention_episodic_days = Some(2);
        m.ledger_retention_days = Some(9);

        assert_eq!(m.layer_retention_days("working"), 7);
        assert_eq!(m.layer_retention_days("episodic"), 2);
        assert_eq!(m.layer_retention_days("semantic"), 30);
        assert_eq!(m.ledger_retention_or_default(), 9);
    }

    #[test]
    fn channels_config_default() {
        let c = ChannelsConfig::default();
        assert!(c.cli);
        assert!(c.telegram.is_none());
        assert!(c.discord.is_none());
    }

    #[test]
    fn persona_config_defaults() {
        let p = PersonaConfig::default();
        assert!(!p.enabled_main_session);
        assert_eq!(p.state_mirror_filename, "STATE.md");
        assert_eq!(p.max_open_loops, 7);
        assert_eq!(p.max_next_actions, 3);
        assert_eq!(p.max_commitments, 5);
        assert_eq!(p.max_current_objective_chars, 280);
        assert_eq!(p.max_recent_context_summary_chars, 1_200);
        assert_eq!(p.max_list_item_chars, 240);
    }

    // ── Serde round-trip ─────────────────────────────────────

    #[test]
    fn config_toml_roundtrip() {
        let config = Config {
            workspace_dir: PathBuf::from("/tmp/test/workspace"),
            config_path: PathBuf::from("/tmp/test/config.toml"),
            api_key: Some("sk-test-key".into()),
            default_provider: Some("openrouter".into()),
            default_model: Some("gpt-4o".into()),
            default_temperature: 0.5,
            observability: ObservabilityConfig {
                backend: "log".into(),
            },
            autonomy: AutonomyConfig {
                level: AutonomyLevel::Full,
                external_action_execution: ExternalActionExecution::Enabled,
                rollout: AutonomyRolloutConfig {
                    stage: AutonomyRolloutStage::Sanitize,
                    verify_repair_enabled: true,
                    contradiction_weighting_enabled: true,
                    intent_audit_anomaly_detection_enabled: true,
                },
                workspace_only: false,
                allowed_commands: vec!["docker".into()],
                forbidden_paths: vec!["/secret".into()],
                max_actions_per_hour: 50,
                max_cost_per_day_cents: 1000,
                verify_repair_max_attempts: 5,
                verify_repair_max_repair_depth: 3,
                temperature_bands: TemperatureBandsConfig::default(),
            },
            runtime: RuntimeConfig {
                kind: "docker".into(),
                enable_docker_runtime: true,
            },
            reliability: ReliabilityConfig::default(),
            heartbeat: HeartbeatConfig {
                enabled: true,
                interval_minutes: 15,
            },
            channels_config: ChannelsConfig {
                cli: true,
                telegram: Some(TelegramConfig {
                    bot_token: "123:ABC".into(),
                    allowed_users: vec!["user1".into()],
                }),
                discord: None,
                slack: None,
                webhook: None,
                imessage: None,
                matrix: None,
                whatsapp: None,
                email: None,
                irc: None,
            },
            memory: MemoryConfig::default(),
            tunnel: TunnelConfig::default(),
            gateway: GatewayConfig::default(),
            composio: ComposioConfig::default(),
            secrets: SecretsConfig::default(),
            browser: BrowserConfig::default(),
            persona: PersonaConfig::default(),
            identity: IdentityConfig::default(),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.api_key, config.api_key);
        assert_eq!(parsed.default_provider, config.default_provider);
        assert_eq!(parsed.default_model, config.default_model);
        assert!((parsed.default_temperature - config.default_temperature).abs() < f64::EPSILON);
        assert_eq!(parsed.observability.backend, "log");
        assert_eq!(parsed.autonomy.level, AutonomyLevel::Full);
        assert_eq!(
            parsed.autonomy.external_action_execution,
            ExternalActionExecution::Enabled
        );
        assert_eq!(
            parsed.autonomy.rollout.stage,
            AutonomyRolloutStage::Sanitize
        );
        assert!(parsed.autonomy.rollout.verify_repair_enabled);
        assert!(parsed.autonomy.rollout.contradiction_weighting_enabled);
        assert!(
            parsed
                .autonomy
                .rollout
                .intent_audit_anomaly_detection_enabled
        );
        assert!(!parsed.autonomy.workspace_only);
        assert_eq!(parsed.runtime.kind, "docker");
        assert!(parsed.runtime.enable_docker_runtime);
        assert!(parsed.heartbeat.enabled);
        assert_eq!(parsed.heartbeat.interval_minutes, 15);
        assert!(parsed.channels_config.telegram.is_some());
        assert_eq!(
            parsed.channels_config.telegram.unwrap().bot_token,
            "123:ABC"
        );
    }

    #[test]
    fn config_minimal_toml_uses_defaults() {
        let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
        let parsed: Config = toml::from_str(minimal).unwrap();
        assert!(parsed.api_key.is_none());
        assert!(parsed.default_provider.is_none());
        assert_eq!(parsed.observability.backend, "none");
        assert_eq!(parsed.autonomy.level, AutonomyLevel::Supervised);
        assert_eq!(
            parsed.autonomy.external_action_execution,
            ExternalActionExecution::Disabled
        );
        assert_eq!(parsed.autonomy.rollout.stage, AutonomyRolloutStage::Off);
        assert!(!parsed.autonomy.rollout.verify_repair_enabled);
        assert!(!parsed.autonomy.rollout.contradiction_weighting_enabled);
        assert!(
            !parsed
                .autonomy
                .rollout
                .intent_audit_anomaly_detection_enabled
        );
        assert_eq!(parsed.runtime.kind, "native");
        assert!(!parsed.heartbeat.enabled);
        assert!(parsed.channels_config.cli);
        assert!(parsed.memory.hygiene_enabled);
        assert_eq!(parsed.memory.archive_after_days, 7);
        assert_eq!(parsed.memory.purge_after_days, 30);
        assert_eq!(parsed.memory.conversation_retention_days, 30);
        assert!(parsed.memory.layer_retention_working_days.is_none());
        assert!(parsed.memory.layer_retention_episodic_days.is_none());
        assert!(parsed.memory.layer_retention_semantic_days.is_none());
        assert!(parsed.memory.layer_retention_procedural_days.is_none());
        assert!(parsed.memory.layer_retention_identity_days.is_none());
        assert!(parsed.memory.ledger_retention_days.is_none());
        assert_eq!(parsed.persona.state_mirror_filename, "STATE.md");
        assert!(!parsed.persona.enabled_main_session);
    }

    #[test]
    fn config_save_and_load_tmpdir() {
        let dir = std::env::temp_dir().join("asteroniris_test_config");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("config.toml");
        let config = Config {
            workspace_dir: dir.join("workspace"),
            config_path: config_path.clone(),
            api_key: Some("sk-roundtrip".into()),
            default_provider: Some("openrouter".into()),
            default_model: Some("test-model".into()),
            default_temperature: 0.9,
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
        };

        config.save().unwrap();
        assert!(config_path.exists());

        let contents = fs::read_to_string(&config_path).unwrap();
        assert!(
            contents.contains("enc2:"),
            "saved config should encrypt secrets"
        );
        assert!(
            !contents.contains("sk-roundtrip"),
            "plaintext API key must not appear in config file"
        );

        let mut loaded: Config = toml::from_str(&contents).unwrap();
        loaded.config_path = config_path.clone();
        loaded.workspace_dir = dir.join("workspace");
        let needs_persist = loaded.decrypt_config_secrets_in_place().unwrap();
        assert!(
            !needs_persist,
            "fresh enc2 values should not require migration"
        );

        assert_eq!(loaded.api_key.as_deref(), Some("sk-roundtrip"));
        assert_eq!(loaded.default_model.as_deref(), Some("test-model"));
        assert!((loaded.default_temperature - 0.9).abs() < f64::EPSILON);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_save_plaintext_when_secret_encryption_disabled() {
        let dir = std::env::temp_dir().join("asteroniris_test_config_no_encrypt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let config_path = dir.join("config.toml");
        let config = Config {
            workspace_dir: dir.join("workspace"),
            config_path: config_path.clone(),
            api_key: Some("sk-no-encrypt".into()),
            default_provider: Some("openrouter".into()),
            default_model: Some("test-model".into()),
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
            secrets: SecretsConfig { encrypt: false },
            browser: BrowserConfig::default(),
            persona: PersonaConfig::default(),
            identity: IdentityConfig::default(),
        };

        config.save().unwrap();

        let contents = fs::read_to_string(&config_path).unwrap();
        assert!(contents.contains("sk-no-encrypt"));
        assert!(!contents.contains("enc2:"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decrypt_config_secrets_marks_plaintext_for_persist_when_encryption_enabled() {
        let dir = std::env::temp_dir().join("asteroniris_test_secret_persist_flag");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut config = Config {
            workspace_dir: dir.join("workspace"),
            config_path: dir.join("config.toml"),
            api_key: Some("sk-plaintext".into()),
            default_provider: Some("openrouter".into()),
            default_model: Some("test-model".into()),
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
        };

        let needs_persist = config.decrypt_config_secrets_in_place().unwrap();
        assert!(needs_persist);
        assert_eq!(config.api_key.as_deref(), Some("sk-plaintext"));

        let _ = fs::remove_dir_all(&dir);
    }

    // ── Telegram / Discord config ────────────────────────────

    #[test]
    fn telegram_config_serde() {
        let tc = TelegramConfig {
            bot_token: "123:XYZ".into(),
            allowed_users: vec!["alice".into(), "bob".into()],
        };
        let json = serde_json::to_string(&tc).unwrap();
        let parsed: TelegramConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.bot_token, "123:XYZ");
        assert_eq!(parsed.allowed_users.len(), 2);
    }

    #[test]
    fn discord_config_serde() {
        let dc = DiscordConfig {
            bot_token: "discord-token".into(),
            guild_id: Some("12345".into()),
            allowed_users: vec![],
        };
        let json = serde_json::to_string(&dc).unwrap();
        let parsed: DiscordConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.bot_token, "discord-token");
        assert_eq!(parsed.guild_id.as_deref(), Some("12345"));
    }

    #[test]
    fn discord_config_optional_guild() {
        let dc = DiscordConfig {
            bot_token: "tok".into(),
            guild_id: None,
            allowed_users: vec![],
        };
        let json = serde_json::to_string(&dc).unwrap();
        let parsed: DiscordConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.guild_id.is_none());
    }

    // ── iMessage / Matrix config ────────────────────────────

    #[test]
    fn imessage_config_serde() {
        let ic = IMessageConfig {
            allowed_contacts: vec!["+1234567890".into(), "user@icloud.com".into()],
        };
        let json = serde_json::to_string(&ic).unwrap();
        let parsed: IMessageConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.allowed_contacts.len(), 2);
        assert_eq!(parsed.allowed_contacts[0], "+1234567890");
    }

    #[test]
    fn imessage_config_empty_contacts() {
        let ic = IMessageConfig {
            allowed_contacts: vec![],
        };
        let json = serde_json::to_string(&ic).unwrap();
        let parsed: IMessageConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.allowed_contacts.is_empty());
    }

    #[test]
    fn imessage_config_wildcard() {
        let ic = IMessageConfig {
            allowed_contacts: vec!["*".into()],
        };
        let toml_str = toml::to_string(&ic).unwrap();
        let parsed: IMessageConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.allowed_contacts, vec!["*"]);
    }

    #[test]
    fn matrix_config_serde() {
        let mc = MatrixConfig {
            homeserver: "https://matrix.org".into(),
            access_token: "syt_token_abc".into(),
            room_id: "!room123:matrix.org".into(),
            allowed_users: vec!["@user:matrix.org".into()],
        };
        let json = serde_json::to_string(&mc).unwrap();
        let parsed: MatrixConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.homeserver, "https://matrix.org");
        assert_eq!(parsed.access_token, "syt_token_abc");
        assert_eq!(parsed.room_id, "!room123:matrix.org");
        assert_eq!(parsed.allowed_users.len(), 1);
    }

    #[test]
    fn matrix_config_toml_roundtrip() {
        let mc = MatrixConfig {
            homeserver: "https://synapse.local:8448".into(),
            access_token: "tok".into(),
            room_id: "!abc:synapse.local".into(),
            allowed_users: vec!["@admin:synapse.local".into(), "*".into()],
        };
        let toml_str = toml::to_string(&mc).unwrap();
        let parsed: MatrixConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.homeserver, "https://synapse.local:8448");
        assert_eq!(parsed.allowed_users.len(), 2);
    }

    #[test]
    fn channels_config_with_imessage_and_matrix() {
        let c = ChannelsConfig {
            cli: true,
            telegram: None,
            discord: None,
            slack: None,
            webhook: None,
            imessage: Some(IMessageConfig {
                allowed_contacts: vec!["+1".into()],
            }),
            matrix: Some(MatrixConfig {
                homeserver: "https://m.org".into(),
                access_token: "tok".into(),
                room_id: "!r:m".into(),
                allowed_users: vec!["@u:m".into()],
            }),
            whatsapp: None,
            email: None,
            irc: None,
        };
        let toml_str = toml::to_string_pretty(&c).unwrap();
        let parsed: ChannelsConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.imessage.is_some());
        assert!(parsed.matrix.is_some());
        assert_eq!(parsed.imessage.unwrap().allowed_contacts, vec!["+1"]);
        assert_eq!(parsed.matrix.unwrap().homeserver, "https://m.org");
    }

    #[test]
    fn channels_config_default_has_no_imessage_matrix() {
        let c = ChannelsConfig::default();
        assert!(c.imessage.is_none());
        assert!(c.matrix.is_none());
    }

    // ── Edge cases: serde(default) for allowed_users ─────────

    #[test]
    fn discord_config_deserializes_without_allowed_users() {
        // Old configs won't have allowed_users — serde(default) should fill vec![]
        let json = r#"{"bot_token":"tok","guild_id":"123"}"#;
        let parsed: DiscordConfig = serde_json::from_str(json).unwrap();
        assert!(parsed.allowed_users.is_empty());
    }

    #[test]
    fn discord_config_deserializes_with_allowed_users() {
        let json = r#"{"bot_token":"tok","guild_id":"123","allowed_users":["111","222"]}"#;
        let parsed: DiscordConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.allowed_users, vec!["111", "222"]);
    }

    #[test]
    fn slack_config_deserializes_without_allowed_users() {
        let json = r#"{"bot_token":"xoxb-tok"}"#;
        let parsed: SlackConfig = serde_json::from_str(json).unwrap();
        assert!(parsed.allowed_users.is_empty());
    }

    #[test]
    fn slack_config_deserializes_with_allowed_users() {
        let json = r#"{"bot_token":"xoxb-tok","allowed_users":["U111"]}"#;
        let parsed: SlackConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.allowed_users, vec!["U111"]);
    }

    #[test]
    fn discord_config_toml_backward_compat() {
        let toml_str = r#"
bot_token = "tok"
guild_id = "123"
"#;
        let parsed: DiscordConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.allowed_users.is_empty());
        assert_eq!(parsed.bot_token, "tok");
    }

    #[test]
    fn slack_config_toml_backward_compat() {
        let toml_str = r#"
bot_token = "xoxb-tok"
channel_id = "C123"
"#;
        let parsed: SlackConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.allowed_users.is_empty());
        assert_eq!(parsed.channel_id.as_deref(), Some("C123"));
    }

    #[test]
    fn webhook_config_with_secret() {
        let json = r#"{"port":8080,"secret":"my-secret-key"}"#;
        let parsed: WebhookConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.secret.as_deref(), Some("my-secret-key"));
    }

    #[test]
    fn webhook_config_without_secret() {
        let json = r#"{"port":8080}"#;
        let parsed: WebhookConfig = serde_json::from_str(json).unwrap();
        assert!(parsed.secret.is_none());
        assert_eq!(parsed.port, 8080);
    }

    // ── WhatsApp config ──────────────────────────────────────

    #[test]
    fn whatsapp_config_serde() {
        let wc = WhatsAppConfig {
            access_token: "EAABx...".into(),
            phone_number_id: "123456789".into(),
            verify_token: "my-verify-token".into(),
            app_secret: None,
            allowed_numbers: vec!["+1234567890".into(), "+9876543210".into()],
        };
        let json = serde_json::to_string(&wc).unwrap();
        let parsed: WhatsAppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token, "EAABx...");
        assert_eq!(parsed.phone_number_id, "123456789");
        assert_eq!(parsed.verify_token, "my-verify-token");
        assert_eq!(parsed.allowed_numbers.len(), 2);
    }

    #[test]
    fn whatsapp_config_toml_roundtrip() {
        let wc = WhatsAppConfig {
            access_token: "tok".into(),
            phone_number_id: "12345".into(),
            verify_token: "verify".into(),
            app_secret: None,
            allowed_numbers: vec!["+1".into()],
        };
        let toml_str = toml::to_string(&wc).unwrap();
        let parsed: WhatsAppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.phone_number_id, "12345");
        assert_eq!(parsed.allowed_numbers, vec!["+1"]);
    }

    #[test]
    fn whatsapp_config_deserializes_without_allowed_numbers() {
        let json = r#"{"access_token":"tok","phone_number_id":"123","verify_token":"ver"}"#;
        let parsed: WhatsAppConfig = serde_json::from_str(json).unwrap();
        assert!(parsed.allowed_numbers.is_empty());
    }

    #[test]
    fn whatsapp_config_wildcard_allowed() {
        let wc = WhatsAppConfig {
            access_token: "tok".into(),
            phone_number_id: "123".into(),
            verify_token: "ver".into(),
            app_secret: None,
            allowed_numbers: vec!["*".into()],
        };
        let toml_str = toml::to_string(&wc).unwrap();
        let parsed: WhatsAppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.allowed_numbers, vec!["*"]);
    }

    #[test]
    fn channels_config_with_whatsapp() {
        let c = ChannelsConfig {
            cli: true,
            telegram: None,
            discord: None,
            slack: None,
            webhook: None,
            imessage: None,
            matrix: None,
            whatsapp: Some(WhatsAppConfig {
                access_token: "tok".into(),
                phone_number_id: "123".into(),
                verify_token: "ver".into(),
                app_secret: None,
                allowed_numbers: vec!["+1".into()],
            }),
            email: None,
            irc: None,
        };
        let toml_str = toml::to_string_pretty(&c).unwrap();
        let parsed: ChannelsConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.whatsapp.is_some());
        let wa = parsed.whatsapp.unwrap();
        assert_eq!(wa.phone_number_id, "123");
        assert_eq!(wa.allowed_numbers, vec!["+1"]);
    }

    #[test]
    fn channels_config_default_has_no_whatsapp() {
        let c = ChannelsConfig::default();
        assert!(c.whatsapp.is_none());
    }

    // ══════════════════════════════════════════════════════════
    // SECURITY CHECKLIST TESTS — Gateway config
    // ══════════════════════════════════════════════════════════

    #[test]
    fn checklist_gateway_default_requires_pairing() {
        let g = GatewayConfig::default();
        assert!(g.require_pairing, "Pairing must be required by default");
    }

    #[test]
    fn checklist_gateway_default_blocks_public_bind() {
        let g = GatewayConfig::default();
        assert!(
            !g.allow_public_bind,
            "Public bind must be blocked by default"
        );
    }

    #[test]
    fn checklist_gateway_default_no_tokens() {
        let g = GatewayConfig::default();
        assert!(
            g.paired_tokens.is_empty(),
            "No pre-paired tokens by default"
        );
    }

    #[test]
    fn checklist_gateway_default_defense_mode_is_enforce() {
        let g = GatewayConfig::default();
        assert_eq!(g.defense_mode, GatewayDefenseMode::Enforce);
        assert!(!g.defense_kill_switch);
    }

    #[test]
    fn checklist_gateway_cli_default_host_is_localhost() {
        // The CLI default for --host is 127.0.0.1 (checked in main.rs)
        // Here we verify the config default matches
        let c = Config::default();
        assert!(
            c.gateway.require_pairing,
            "Config default must require pairing"
        );
        assert!(
            !c.gateway.allow_public_bind,
            "Config default must block public bind"
        );
    }

    #[test]
    fn checklist_gateway_serde_roundtrip() {
        let g = GatewayConfig {
            port: 3000,
            host: "127.0.0.1".into(),
            require_pairing: true,
            allow_public_bind: false,
            paired_tokens: vec!["zc_test_token".into()],
            defense_mode: GatewayDefenseMode::Warn,
            defense_kill_switch: true,
        };
        let toml_str = toml::to_string(&g).unwrap();
        let parsed: GatewayConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.require_pairing);
        assert!(!parsed.allow_public_bind);
        assert_eq!(parsed.paired_tokens, vec!["zc_test_token"]);
        assert_eq!(parsed.defense_mode, GatewayDefenseMode::Warn);
        assert!(parsed.defense_kill_switch);
    }

    #[test]
    fn checklist_gateway_backward_compat_no_gateway_section() {
        // Old configs without [gateway] should get secure defaults
        let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
        let parsed: Config = toml::from_str(minimal).unwrap();
        assert!(
            parsed.gateway.require_pairing,
            "Missing [gateway] must default to require_pairing=true"
        );
        assert!(
            !parsed.gateway.allow_public_bind,
            "Missing [gateway] must default to allow_public_bind=false"
        );
        assert_eq!(parsed.gateway.defense_mode, GatewayDefenseMode::Enforce);
        assert!(!parsed.gateway.defense_kill_switch);
    }

    #[test]
    fn checklist_autonomy_default_is_workspace_scoped() {
        let a = AutonomyConfig::default();
        assert!(a.workspace_only, "Default autonomy must be workspace_only");
        assert!(
            a.forbidden_paths.contains(&"/etc".to_string()),
            "Must block /etc"
        );
        assert!(
            a.forbidden_paths.contains(&"/proc".to_string()),
            "Must block /proc"
        );
        assert!(
            a.forbidden_paths.contains(&"~/.ssh".to_string()),
            "Must block ~/.ssh"
        );
    }

    // ══════════════════════════════════════════════════════════
    // COMPOSIO CONFIG TESTS
    // ══════════════════════════════════════════════════════════

    #[test]
    fn composio_config_default_disabled() {
        let c = ComposioConfig::default();
        assert!(!c.enabled, "Composio must be disabled by default");
        assert!(c.api_key.is_none(), "No API key by default");
        assert_eq!(c.entity_id, "default");
    }

    #[test]
    fn composio_config_serde_roundtrip() {
        let c = ComposioConfig {
            enabled: true,
            api_key: Some("comp-key-123".into()),
            entity_id: "user42".into(),
        };
        let toml_str = toml::to_string(&c).unwrap();
        let parsed: ComposioConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.api_key.as_deref(), Some("comp-key-123"));
        assert_eq!(parsed.entity_id, "user42");
    }

    #[test]
    fn composio_config_backward_compat_missing_section() {
        let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
        let parsed: Config = toml::from_str(minimal).unwrap();
        assert!(
            !parsed.composio.enabled,
            "Missing [composio] must default to disabled"
        );
        assert!(parsed.composio.api_key.is_none());
    }

    #[test]
    fn composio_config_partial_toml() {
        let toml_str = r"
enabled = true
";
        let parsed: ComposioConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.enabled);
        assert!(parsed.api_key.is_none());
        assert_eq!(parsed.entity_id, "default");
    }

    // ══════════════════════════════════════════════════════════
    // SECRETS CONFIG TESTS
    // ══════════════════════════════════════════════════════════

    #[test]
    fn secrets_config_default_encrypts() {
        let s = SecretsConfig::default();
        assert!(s.encrypt, "Encryption must be enabled by default");
    }

    #[test]
    fn secrets_config_serde_roundtrip() {
        let s = SecretsConfig { encrypt: false };
        let toml_str = toml::to_string(&s).unwrap();
        let parsed: SecretsConfig = toml::from_str(&toml_str).unwrap();
        assert!(!parsed.encrypt);
    }

    #[test]
    fn secrets_config_backward_compat_missing_section() {
        let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
        let parsed: Config = toml::from_str(minimal).unwrap();
        assert!(
            parsed.secrets.encrypt,
            "Missing [secrets] must default to encrypt=true"
        );
    }

    #[test]
    fn config_default_has_composio_and_secrets() {
        let c = Config::default();
        assert!(!c.composio.enabled);
        assert!(c.composio.api_key.is_none());
        assert!(c.secrets.encrypt);
        assert!(!c.browser.enabled);
        assert!(c.browser.allowed_domains.is_empty());
    }

    #[test]
    fn browser_config_default_disabled() {
        let b = BrowserConfig::default();
        assert!(!b.enabled);
        assert!(b.allowed_domains.is_empty());
    }

    #[test]
    fn browser_config_serde_roundtrip() {
        let b = BrowserConfig {
            enabled: true,
            allowed_domains: vec!["example.com".into(), "docs.example.com".into()],
            session_name: None,
        };
        let toml_str = toml::to_string(&b).unwrap();
        let parsed: BrowserConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.allowed_domains.len(), 2);
        assert_eq!(parsed.allowed_domains[0], "example.com");
    }

    #[test]
    fn browser_config_backward_compat_missing_section() {
        let minimal = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7
"#;
        let parsed: Config = toml::from_str(minimal).unwrap();
        assert!(!parsed.browser.enabled);
        assert!(parsed.browser.allowed_domains.is_empty());
    }

    // ── Environment variable overrides (Docker support) ─────────

    #[test]
    fn env_override_api_key() {
        let _guard = env_lock();
        let mut config = Config::default();
        assert!(config.api_key.is_none());

        unsafe {
            std::env::set_var("ASTERONIRIS_API_KEY", "sk-test-env-key");
        }
        config.apply_env_overrides();
        assert_eq!(config.api_key.as_deref(), Some("sk-test-env-key"));

        unsafe {
            std::env::remove_var("ASTERONIRIS_API_KEY");
        }
    }

    #[test]
    fn env_override_api_key_fallback() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::remove_var("ASTERONIRIS_API_KEY");
            std::env::set_var("API_KEY", "sk-fallback-key");
        }
        config.apply_env_overrides();
        assert_eq!(config.api_key.as_deref(), Some("sk-fallback-key"));

        unsafe {
            std::env::remove_var("API_KEY");
        }
    }

    #[test]
    fn env_override_provider() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::set_var("ASTERONIRIS_PROVIDER", "anthropic");
        }
        config.apply_env_overrides();
        assert_eq!(config.default_provider.as_deref(), Some("anthropic"));

        unsafe {
            std::env::remove_var("ASTERONIRIS_PROVIDER");
        }
    }

    #[test]
    fn env_override_provider_fallback() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::remove_var("ASTERONIRIS_PROVIDER");
            std::env::set_var("PROVIDER", "openai");
        }
        config.apply_env_overrides();
        assert_eq!(config.default_provider.as_deref(), Some("openai"));

        unsafe {
            std::env::remove_var("PROVIDER");
        }
    }

    #[test]
    fn env_override_model() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::set_var("ASTERONIRIS_MODEL", "gpt-4o");
        }
        config.apply_env_overrides();
        assert_eq!(config.default_model.as_deref(), Some("gpt-4o"));

        unsafe {
            std::env::remove_var("ASTERONIRIS_MODEL");
        }
    }

    #[test]
    fn env_override_workspace() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::set_var("ASTERONIRIS_WORKSPACE", "/custom/workspace");
        }
        config.apply_env_overrides();
        assert_eq!(config.workspace_dir, PathBuf::from("/custom/workspace"));

        unsafe {
            std::env::remove_var("ASTERONIRIS_WORKSPACE");
        }
    }

    #[test]
    fn env_override_empty_values_ignored() {
        let _guard = env_lock();
        let mut config = Config::default();
        let original_provider = config.default_provider.clone();

        unsafe {
            std::env::set_var("ASTERONIRIS_PROVIDER", "");
        }
        config.apply_env_overrides();
        assert_eq!(config.default_provider, original_provider);

        unsafe {
            std::env::remove_var("ASTERONIRIS_PROVIDER");
        }
    }

    #[test]
    fn env_override_gateway_port() {
        let _guard = env_lock();
        let mut config = Config::default();
        assert_eq!(config.gateway.port, 3000);

        unsafe {
            std::env::set_var("ASTERONIRIS_GATEWAY_PORT", "8080");
        }
        config.apply_env_overrides();
        assert_eq!(config.gateway.port, 8080);

        unsafe {
            std::env::remove_var("ASTERONIRIS_GATEWAY_PORT");
        }
    }

    #[test]
    fn env_override_port_fallback() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::remove_var("ASTERONIRIS_GATEWAY_PORT");
            std::env::set_var("PORT", "9000");
        }
        config.apply_env_overrides();
        assert_eq!(config.gateway.port, 9000);

        unsafe {
            std::env::remove_var("PORT");
        }
    }

    #[test]
    fn env_override_gateway_host() {
        let _guard = env_lock();
        let mut config = Config::default();
        assert_eq!(config.gateway.host, "127.0.0.1");

        unsafe {
            std::env::set_var("ASTERONIRIS_GATEWAY_HOST", "0.0.0.0");
        }
        config.apply_env_overrides();
        assert_eq!(config.gateway.host, "0.0.0.0");

        unsafe {
            std::env::remove_var("ASTERONIRIS_GATEWAY_HOST");
        }
    }

    #[test]
    fn env_override_host_fallback() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::remove_var("ASTERONIRIS_GATEWAY_HOST");
            std::env::set_var("HOST", "0.0.0.0");
        }
        config.apply_env_overrides();
        assert_eq!(config.gateway.host, "0.0.0.0");

        unsafe {
            std::env::remove_var("HOST");
        }
    }

    #[test]
    fn env_override_temperature() {
        let _guard = env_lock();
        let mut config = Config::default();

        unsafe {
            std::env::set_var("ASTERONIRIS_TEMPERATURE", "0.5");
        }
        config.apply_env_overrides();
        assert!((config.default_temperature - 0.5).abs() < f64::EPSILON);

        unsafe {
            std::env::remove_var("ASTERONIRIS_TEMPERATURE");
        }
    }

    #[test]
    fn env_override_temperature_out_of_range_ignored() {
        let _guard = env_lock();
        // Clean up any leftover env vars from other tests
        unsafe {
            std::env::remove_var("ASTERONIRIS_TEMPERATURE");
        }

        let mut config = Config::default();
        let original_temp = config.default_temperature;

        // Temperature > 2.0 should be ignored
        unsafe {
            std::env::set_var("ASTERONIRIS_TEMPERATURE", "3.0");
        }
        config.apply_env_overrides();
        assert!(
            (config.default_temperature - original_temp).abs() < f64::EPSILON,
            "Temperature 3.0 should be ignored (out of range)"
        );

        unsafe {
            std::env::remove_var("ASTERONIRIS_TEMPERATURE");
        }
    }

    #[test]
    fn config_temperature_band_validation() {
        let config = Config::default();
        assert!(config.validate_temperature_bands().is_ok());

        let mut invalid = Config::default();
        invalid.autonomy.temperature_bands.full.min = 1.3;
        invalid.autonomy.temperature_bands.full.max = 1.2;

        let err = invalid.validate_temperature_bands().unwrap_err();
        assert!(err
            .to_string()
            .contains("autonomy.temperature_bands.full min must be <= max"));
    }

    #[test]
    fn config_verify_repair_caps_validation() {
        let config = Config::default();
        assert!(config.autonomy.validate_verify_repair_caps().is_ok());

        let mut invalid_attempts = Config::default();
        invalid_attempts.autonomy.verify_repair_max_attempts = 0;
        let err = invalid_attempts
            .autonomy
            .validate_verify_repair_caps()
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("autonomy.verify_repair_max_attempts must be >= 1"));

        let mut invalid_depth = Config::default();
        invalid_depth.autonomy.verify_repair_max_attempts = 2;
        invalid_depth.autonomy.verify_repair_max_repair_depth = 2;
        let err = invalid_depth
            .autonomy
            .validate_verify_repair_caps()
            .unwrap_err();
        assert!(err.to_string().contains(
            "autonomy.verify_repair_max_repair_depth must be < autonomy.verify_repair_max_attempts"
        ));
    }

    #[test]
    fn autonomy_rollout_stage_toml_deserialization() {
        let toml = r#"
workspace_dir = "/tmp/ws"
config_path = "/tmp/config.toml"
default_temperature = 0.7

[autonomy]
level = "supervised"
external_action_execution = "disabled"
workspace_only = true
allowed_commands = ["cargo"]
forbidden_paths = ["/etc"]
max_actions_per_hour = 20
max_cost_per_day_cents = 500

[autonomy.rollout]
stage = "audit-only"
verify_repair_enabled = false
contradiction_weighting_enabled = false
intent_audit_anomaly_detection_enabled = true
"#;

        let parsed: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            parsed.autonomy.rollout.stage,
            AutonomyRolloutStage::AuditOnly
        );
        assert!(!parsed.autonomy.rollout.verify_repair_enabled);
        assert!(!parsed.autonomy.rollout.contradiction_weighting_enabled);
        assert!(
            parsed
                .autonomy
                .rollout
                .intent_audit_anomaly_detection_enabled
        );
    }

    #[test]
    fn env_override_invalid_port_ignored() {
        let _guard = env_lock();
        let mut config = Config::default();
        let original_port = config.gateway.port;

        unsafe {
            std::env::set_var("PORT", "not_a_number");
        }
        config.apply_env_overrides();
        assert_eq!(config.gateway.port, original_port);

        unsafe {
            std::env::remove_var("PORT");
        }
    }

    #[test]
    fn gateway_config_default_values() {
        let g = GatewayConfig::default();
        assert_eq!(g.port, 3000);
        assert_eq!(g.host, "127.0.0.1");
        assert!(g.require_pairing);
        assert!(!g.allow_public_bind);
        assert!(g.paired_tokens.is_empty());
        assert_eq!(g.defense_mode, GatewayDefenseMode::Enforce);
        assert!(!g.defense_kill_switch);
    }
}
