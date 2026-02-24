use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
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
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleTunnelConfig {
    #[serde(default)]
    pub funnel: bool,
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NgrokTunnelConfig {
    pub auth_token: String,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTunnelConfig {
    pub start_command: String,
    pub health_url: Option<String>,
    pub url_pattern: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tunnel_config() {
        let config = TunnelConfig::default();
        assert_eq!(config.provider, "none");
        assert!(config.cloudflare.is_none());
        assert!(config.tailscale.is_none());
        assert!(config.ngrok.is_none());
        assert!(config.custom.is_none());
    }

    #[test]
    fn tunnel_config_toml_round_trip() {
        let original = TunnelConfig {
            provider: "custom".into(),
            cloudflare: Some(CloudflareTunnelConfig {
                token: "cf-token".into(),
            }),
            tailscale: Some(TailscaleTunnelConfig {
                funnel: true,
                hostname: Some("agent.tailnet.ts.net".into()),
            }),
            ngrok: Some(NgrokTunnelConfig {
                auth_token: "ngrok-token".into(),
                domain: Some("agent.ngrok.app".into()),
            }),
            custom: Some(CustomTunnelConfig {
                start_command: "bore local {port} --to bore.pub".into(),
                health_url: Some("https://agent.example/health".into()),
                url_pattern: Some("https://[^\\s]+".into()),
            }),
        };
        let toml = toml::to_string(&original).unwrap();
        let decoded: TunnelConfig = toml::from_str(&toml).unwrap();
        assert_eq!(decoded.provider, original.provider);
    }
}
