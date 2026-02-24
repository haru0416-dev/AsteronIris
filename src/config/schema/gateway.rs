use serde::{Deserialize, Serialize};

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
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default = "default_gateway_host")]
    pub host: String,
    #[serde(default = "default_true")]
    pub require_pairing: bool,
    #[serde(default)]
    pub allow_public_bind: bool,
    #[serde(default)]
    pub paired_tokens: Vec<String>,
    #[serde(default = "default_token_ttl_secs")]
    pub token_ttl_secs: u64,
    #[serde(default)]
    pub defense_mode: GatewayDefenseMode,
    #[serde(default)]
    pub defense_kill_switch: bool,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    #[serde(default)]
    pub openai_compat_api_keys: Vec<String>,
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
fn default_token_ttl_secs() -> u64 {
    2_592_000
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: default_gateway_port(),
            host: default_gateway_host(),
            require_pairing: true,
            allow_public_bind: false,
            paired_tokens: Vec::new(),
            token_ttl_secs: default_token_ttl_secs(),
            defense_mode: GatewayDefenseMode::default(),
            defense_kill_switch: false,
            cors_origins: Vec::new(),
            openai_compat_api_keys: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gateway_config() {
        let config = GatewayConfig::default();
        assert_eq!(config.port, 3000);
        assert_eq!(config.host, "127.0.0.1");
        assert!(config.require_pairing);
        assert!(!config.allow_public_bind);
        assert_eq!(config.token_ttl_secs, 2_592_000);
        assert_eq!(config.defense_mode, GatewayDefenseMode::Enforce);
        assert!(config.cors_origins.is_empty());
        assert!(config.openai_compat_api_keys.is_empty());
    }

    #[test]
    fn gateway_config_toml_round_trip() {
        let original = GatewayConfig {
            port: 4001,
            host: "0.0.0.0".into(),
            require_pairing: false,
            allow_public_bind: true,
            paired_tokens: vec!["alpha".into(), "beta".into()],
            token_ttl_secs: 600,
            defense_mode: GatewayDefenseMode::Warn,
            defense_kill_switch: true,
            cors_origins: vec!["https://example.com".into()],
            openai_compat_api_keys: vec!["test-openai-key".into()],
        };
        let toml = toml::to_string(&original).unwrap();
        let decoded: GatewayConfig = toml::from_str(&toml).unwrap();
        assert_eq!(decoded.port, original.port);
        assert_eq!(decoded.host, original.host);
        assert_eq!(decoded.require_pairing, original.require_pairing);
        assert_eq!(
            decoded.openai_compat_api_keys,
            original.openai_compat_api_keys
        );
    }
}
