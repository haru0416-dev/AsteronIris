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
    #[serde(default = "default_token_ttl_secs")]
    pub token_ttl_secs: u64,
    #[serde(default)]
    pub defense_mode: GatewayDefenseMode,
    #[serde(default)]
    pub defense_kill_switch: bool,
    /// Allowed CORS origins. Empty = deny all cross-origin requests (default).
    #[serde(default)]
    pub cors_origins: Vec<String>,
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
    }

    #[test]
    fn gateway_defense_mode_serde_variants() {
        let cases = [
            (GatewayDefenseMode::Audit, "\"audit\""),
            (GatewayDefenseMode::Warn, "\"warn\""),
            (GatewayDefenseMode::Enforce, "\"enforce\""),
        ];

        for (mode, expected_json) in cases {
            let serialized = serde_json::to_string(&mode).unwrap();
            assert_eq!(serialized, expected_json);

            let deserialized: GatewayDefenseMode = serde_json::from_str(expected_json).unwrap();
            assert_eq!(deserialized, mode);
        }
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
            cors_origins: vec![
                "https://example.com".into(),
                "https://app.example.com".into(),
            ],
        };

        let toml = toml::to_string(&original).unwrap();
        let decoded: GatewayConfig = toml::from_str(&toml).unwrap();

        assert_eq!(decoded.port, original.port);
        assert_eq!(decoded.host, original.host);
        assert_eq!(decoded.require_pairing, original.require_pairing);
        assert_eq!(decoded.allow_public_bind, original.allow_public_bind);
        assert_eq!(decoded.paired_tokens, original.paired_tokens);
        assert_eq!(decoded.token_ttl_secs, original.token_ttl_secs);
        assert_eq!(decoded.defense_mode, original.defense_mode);
        assert_eq!(decoded.defense_kill_switch, original.defense_kill_switch);
        assert_eq!(decoded.cors_origins, original.cors_origins);
    }

    #[test]
    fn gateway_config_defaults_token_ttl_when_missing() {
        let decoded: GatewayConfig = toml::from_str(
            r#"
port = 3000
host = "127.0.0.1"
require_pairing = true
allow_public_bind = false
paired_tokens = []
"#,
        )
        .unwrap();

        assert_eq!(decoded.token_ttl_secs, 2_592_000);
    }
}
