use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_axes")]
    pub axes: Vec<String>,
    #[serde(default = "default_true")]
    pub text_enabled: bool,
    #[serde(default = "default_true")]
    pub ui_enabled: bool,
}

fn default_backend() -> String {
    "llm".into()
}
fn default_axes() -> Vec<String> {
    vec![
        "coherence".into(),
        "hierarchy".into(),
        "intentionality".into(),
    ]
}
fn default_true() -> bool {
    true
}

impl Default for TasteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: default_backend(),
            axes: default_axes(),
            text_enabled: default_true(),
            ui_enabled: default_true(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taste_config_default() {
        let cfg = TasteConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.backend, "llm");
        assert_eq!(cfg.axes.len(), 3);
        assert!(cfg.text_enabled);
        assert!(cfg.ui_enabled);
    }

    #[test]
    fn test_taste_config_toml_roundtrip() {
        let cfg = TasteConfig::default();
        let serialized = toml::to_string(&cfg).expect("serialize");
        let deserialized: TasteConfig = toml::from_str(&serialized).expect("deserialize");
        assert!(!deserialized.enabled);
        assert_eq!(deserialized.backend, "llm");
    }
}
