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
    #[must_use]
    pub fn effective_autonomy_level(&self) -> AutonomyLevel {
        if !self.rollout.enabled {
            return self.level;
        }

        let Some(stage) = self.rollout.stage else {
            return self.level;
        };

        min_autonomy(self.level, rollout_stage_to_autonomy(stage))
    }

    #[must_use]
    pub fn selected_temperature_band(&self) -> TemperatureBand {
        match self.effective_autonomy_level() {
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

#[must_use]
fn rollout_stage_to_autonomy(stage: AutonomyRolloutStage) -> AutonomyLevel {
    match stage {
        AutonomyRolloutStage::ReadOnly => AutonomyLevel::ReadOnly,
        AutonomyRolloutStage::Supervised => AutonomyLevel::Supervised,
        AutonomyRolloutStage::Full => AutonomyLevel::Full,
    }
}

#[must_use]
fn min_autonomy(global: AutonomyLevel, channel: AutonomyLevel) -> AutonomyLevel {
    match (global, channel) {
        (AutonomyLevel::ReadOnly, _) | (_, AutonomyLevel::ReadOnly) => AutonomyLevel::ReadOnly,
        (AutonomyLevel::Supervised, _) | (_, AutonomyLevel::Supervised) => {
            AutonomyLevel::Supervised
        }
        (AutonomyLevel::Full, AutonomyLevel::Full) => AutonomyLevel::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_band_validate_accepts_valid_band() {
        let band = TemperatureBand { min: 0.1, max: 1.8 };
        assert!(band.validate("test").is_ok());
    }

    #[test]
    fn temperature_band_validate_rejects_min_greater_than_max() {
        let band = TemperatureBand { min: 1.0, max: 0.5 };
        assert!(band.validate("test").is_err());
    }

    #[test]
    fn temperature_band_validate_rejects_nan_values() {
        let min_nan = TemperatureBand {
            min: f64::NAN,
            max: 1.0,
        };
        let max_nan = TemperatureBand {
            min: 0.5,
            max: f64::NAN,
        };

        assert!(min_nan.validate("test").is_err());
        assert!(max_nan.validate("test").is_err());
    }

    #[test]
    fn temperature_band_validate_rejects_values_outside_range() {
        let below_range = TemperatureBand {
            min: -0.1,
            max: 0.5,
        };
        let above_range = TemperatureBand { min: 0.5, max: 2.1 };

        assert!(below_range.validate("test").is_err());
        assert!(above_range.validate("test").is_err());
    }

    #[test]
    fn temperature_band_validate_accepts_boundary_values() {
        let boundary = TemperatureBand { min: 0.0, max: 2.0 };
        assert!(boundary.validate("test").is_ok());
    }

    #[test]
    fn selected_temperature_band_matches_autonomy_level() {
        let bands = TemperatureBandsConfig {
            read_only: TemperatureBand { min: 0.0, max: 0.1 },
            supervised: TemperatureBand { min: 0.2, max: 0.3 },
            full: TemperatureBand { min: 0.4, max: 0.9 },
        };

        let read_only_cfg = AutonomyConfig {
            level: AutonomyLevel::ReadOnly,
            temperature_bands: bands.clone(),
            ..AutonomyConfig::default()
        };
        let supervised_cfg = AutonomyConfig {
            level: AutonomyLevel::Supervised,
            temperature_bands: bands.clone(),
            ..AutonomyConfig::default()
        };
        let full_cfg = AutonomyConfig {
            level: AutonomyLevel::Full,
            temperature_bands: bands,
            ..AutonomyConfig::default()
        };

        let read_only_band = read_only_cfg.selected_temperature_band();
        let supervised_band = supervised_cfg.selected_temperature_band();
        let full_band = full_cfg.selected_temperature_band();

        assert_eq!(read_only_band.min, 0.0);
        assert_eq!(read_only_band.max, 0.1);
        assert_eq!(supervised_band.min, 0.2);
        assert_eq!(supervised_band.max, 0.3);
        assert_eq!(full_band.min, 0.4);
        assert_eq!(full_band.max, 0.9);
    }

    #[test]
    fn effective_autonomy_level_rollout_disabled_returns_configured_level() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Full,
            rollout: AutonomyRolloutConfig {
                enabled: false,
                stage: Some(AutonomyRolloutStage::ReadOnly),
                ..AutonomyRolloutConfig::default()
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.effective_autonomy_level(), AutonomyLevel::Full);
    }

    #[test]
    fn effective_autonomy_level_rollout_enabled_read_only_overrides_full() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Full,
            rollout: AutonomyRolloutConfig {
                enabled: true,
                stage: Some(AutonomyRolloutStage::ReadOnly),
                ..AutonomyRolloutConfig::default()
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.effective_autonomy_level(), AutonomyLevel::ReadOnly);
    }

    #[test]
    fn effective_autonomy_level_rollout_enabled_supervised_caps_full() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Full,
            rollout: AutonomyRolloutConfig {
                enabled: true,
                stage: Some(AutonomyRolloutStage::Supervised),
                ..AutonomyRolloutConfig::default()
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.effective_autonomy_level(), AutonomyLevel::Supervised);
    }

    #[test]
    fn effective_autonomy_level_rollout_enabled_without_stage_returns_configured_level() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Supervised,
            rollout: AutonomyRolloutConfig {
                enabled: true,
                stage: None,
                ..AutonomyRolloutConfig::default()
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.effective_autonomy_level(), AutonomyLevel::Supervised);
    }

    #[test]
    fn effective_autonomy_level_rollout_enabled_full_keeps_full_when_config_full() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Full,
            rollout: AutonomyRolloutConfig {
                enabled: true,
                stage: Some(AutonomyRolloutStage::Full),
                ..AutonomyRolloutConfig::default()
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.effective_autonomy_level(), AutonomyLevel::Full);
    }

    #[test]
    fn effective_autonomy_level_rollout_cannot_escalate_supervised_to_full() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Supervised,
            rollout: AutonomyRolloutConfig {
                enabled: true,
                stage: Some(AutonomyRolloutStage::Full),
                ..AutonomyRolloutConfig::default()
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.effective_autonomy_level(), AutonomyLevel::Supervised);
    }

    #[test]
    fn selected_temperature_band_uses_effective_autonomy_level() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Full,
            rollout: AutonomyRolloutConfig {
                enabled: true,
                stage: Some(AutonomyRolloutStage::ReadOnly),
                ..AutonomyRolloutConfig::default()
            },
            temperature_bands: TemperatureBandsConfig {
                read_only: TemperatureBand { min: 0.0, max: 0.1 },
                supervised: TemperatureBand { min: 0.2, max: 0.7 },
                full: TemperatureBand { min: 0.8, max: 1.1 },
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.selected_temperature_band().max, 0.1);
    }

    #[test]
    fn clamp_temperature_applies_band_limits() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Supervised,
            temperature_bands: TemperatureBandsConfig {
                read_only: TemperatureBand { min: 0.0, max: 0.2 },
                supervised: TemperatureBand { min: 0.2, max: 0.7 },
                full: TemperatureBand { min: 0.2, max: 1.0 },
            },
            ..AutonomyConfig::default()
        };

        assert_eq!(config.clamp_temperature(0.5), 0.5);
        assert_eq!(config.clamp_temperature(0.2), 0.2);
        assert_eq!(config.clamp_temperature(0.7), 0.7);
        assert_eq!(config.clamp_temperature(0.1), 0.2);
        assert_eq!(config.clamp_temperature(0.9), 0.7);
    }

    #[test]
    fn validate_temperature_bands_accepts_valid_configuration() {
        let config = AutonomyConfig {
            temperature_bands: TemperatureBandsConfig {
                read_only: TemperatureBand { min: 0.0, max: 0.1 },
                supervised: TemperatureBand { min: 0.1, max: 0.7 },
                full: TemperatureBand { min: 0.1, max: 1.0 },
            },
            ..AutonomyConfig::default()
        };

        assert!(config.validate_temperature_bands().is_ok());
    }

    #[test]
    fn validate_temperature_bands_rejects_invalid_band() {
        let config = AutonomyConfig {
            temperature_bands: TemperatureBandsConfig {
                read_only: TemperatureBand { min: 0.0, max: 0.1 },
                supervised: TemperatureBand { min: 0.9, max: 0.7 },
                full: TemperatureBand { min: 0.1, max: 1.0 },
            },
            ..AutonomyConfig::default()
        };

        assert!(config.validate_temperature_bands().is_err());
    }

    #[test]
    fn validate_verify_repair_caps_accepts_valid_caps() {
        let config = AutonomyConfig {
            verify_repair_max_attempts: 3,
            verify_repair_max_repair_depth: 2,
            ..AutonomyConfig::default()
        };

        assert!(config.validate_verify_repair_caps().is_ok());
    }

    #[test]
    fn validate_verify_repair_caps_rejects_zero_attempts() {
        let config = AutonomyConfig {
            verify_repair_max_attempts: 0,
            verify_repair_max_repair_depth: 0,
            ..AutonomyConfig::default()
        };

        assert!(config.validate_verify_repair_caps().is_err());
    }

    #[test]
    fn validate_verify_repair_caps_rejects_depth_greater_than_or_equal_attempts() {
        let config = AutonomyConfig {
            verify_repair_max_attempts: 3,
            verify_repair_max_repair_depth: 3,
            ..AutonomyConfig::default()
        };

        assert!(config.validate_verify_repair_caps().is_err());
    }

    #[test]
    fn default_config_is_valid_and_has_reasonable_controls() {
        let config = AutonomyConfig::default();

        assert!(config.validate_temperature_bands().is_ok());
        assert!(config.validate_verify_repair_caps().is_ok());
        assert!(config.workspace_only);
        assert!(config.allowed_commands.contains(&"git".to_string()));
        assert!(config.allowed_commands.contains(&"cargo".to_string()));
        assert!(config.forbidden_paths.contains(&"/etc".to_string()));
        assert!(config.max_actions_per_hour > 0);
        assert!(config.max_actions_per_entity_per_hour > 0);
        assert!(config.max_cost_per_day_cents > 0);
    }

    #[test]
    fn autonomy_config_toml_round_trip_preserves_values() {
        let config = AutonomyConfig {
            level: AutonomyLevel::Full,
            external_action_execution: ExternalActionExecution::Enabled,
            workspace_only: false,
            allowed_commands: vec!["git".into(), "cargo".into(), "just".into()],
            forbidden_paths: vec!["/etc".into(), "~/.ssh".into()],
            max_actions_per_hour: 42,
            max_actions_per_entity_per_hour: 11,
            max_cost_per_day_cents: 2_500,
            verify_repair_max_attempts: 5,
            verify_repair_max_repair_depth: 2,
            max_tool_loop_iterations: 12,
            temperature_bands: TemperatureBandsConfig {
                read_only: TemperatureBand { min: 0.0, max: 0.1 },
                supervised: TemperatureBand { min: 0.2, max: 0.6 },
                full: TemperatureBand { min: 0.3, max: 1.2 },
            },
            ..AutonomyConfig::default()
        };

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: AutonomyConfig = toml::from_str(&serialized).unwrap();

        assert!(matches!(deserialized.level, AutonomyLevel::Full));
        assert_eq!(
            deserialized.external_action_execution,
            ExternalActionExecution::Enabled
        );
        assert!(!deserialized.workspace_only);
        assert_eq!(deserialized.allowed_commands, config.allowed_commands);
        assert_eq!(deserialized.forbidden_paths, config.forbidden_paths);
        assert_eq!(deserialized.max_actions_per_hour, 42);
        assert_eq!(deserialized.max_actions_per_entity_per_hour, 11);
        assert_eq!(deserialized.max_cost_per_day_cents, 2_500);
        assert_eq!(deserialized.verify_repair_max_attempts, 5);
        assert_eq!(deserialized.verify_repair_max_repair_depth, 2);
        assert_eq!(deserialized.max_tool_loop_iterations, 12);
        assert_eq!(deserialized.temperature_bands.read_only.min, 0.0);
        assert_eq!(deserialized.temperature_bands.read_only.max, 0.1);
        assert_eq!(deserialized.temperature_bands.supervised.min, 0.2);
        assert_eq!(deserialized.temperature_bands.supervised.max, 0.6);
        assert_eq!(deserialized.temperature_bands.full.min, 0.3);
        assert_eq!(deserialized.temperature_bands.full.max, 1.2);
    }
}
