use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub backend: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            backend: "none".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_observability_config() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.backend, "none");
    }

    #[test]
    fn observability_config_toml_round_trip() {
        let original = ObservabilityConfig {
            backend: "prometheus".into(),
        };
        let toml = toml::to_string(&original).unwrap();
        let decoded: ObservabilityConfig = toml::from_str(&toml).unwrap();
        assert_eq!(decoded.backend, original.backend);
    }
}
