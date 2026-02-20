use crate::security::{AutonomyLevel, ExternalActionExecution};
use anyhow::Result;
use serde::{Deserialize, Serialize};

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
    #[serde(default = "default_max_actions_per_entity_per_hour")]
    pub max_actions_per_entity_per_hour: u32,
    pub max_cost_per_day_cents: u32,
    #[serde(default = "default_verify_repair_max_attempts")]
    pub verify_repair_max_attempts: u32,
    #[serde(default = "default_verify_repair_max_repair_depth")]
    pub verify_repair_max_repair_depth: u32,
    #[serde(default = "default_max_tool_loop_iterations")]
    pub max_tool_loop_iterations: u32,
    #[serde(default)]
    pub temperature_bands: TemperatureBandsConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyRolloutStage {
    ReadOnly,
    Supervised,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyRolloutConfig {
    #[serde(default)]
    pub enabled: bool,
    pub stage: Option<AutonomyRolloutStage>,
    pub read_only_days: Option<u32>,
    pub supervised_days: Option<u32>,
}

impl Default for AutonomyRolloutConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stage: None,
            read_only_days: Some(14),
            supervised_days: Some(14),
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
    TemperatureBand { min: 0.0, max: 0.2 }
}

fn default_temperature_band_supervised() -> TemperatureBand {
    TemperatureBand { min: 0.2, max: 0.7 }
}

fn default_temperature_band_full() -> TemperatureBand {
    TemperatureBand { min: 0.2, max: 1.0 }
}

fn default_verify_repair_max_attempts() -> u32 {
    3
}

fn default_verify_repair_max_repair_depth() -> u32 {
    2
}

fn default_max_tool_loop_iterations() -> u32 {
    10
}

fn default_max_actions_per_entity_per_hour() -> u32 {
    20
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
            max_actions_per_entity_per_hour: default_max_actions_per_entity_per_hour(),
            max_cost_per_day_cents: 500,
            verify_repair_max_attempts: default_verify_repair_max_attempts(),
            verify_repair_max_repair_depth: default_verify_repair_max_repair_depth(),
            max_tool_loop_iterations: default_max_tool_loop_iterations(),
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
