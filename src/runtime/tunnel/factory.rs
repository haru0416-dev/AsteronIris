use super::{CloudflareTunnel, CustomTunnel, NgrokTunnel, TailscaleTunnel, Tunnel};
use crate::config::schema::{TailscaleTunnelConfig, TunnelConfig};
use anyhow::{Result, bail};

/// Create a tunnel from config. Returns `None` for provider "none".
pub fn create_tunnel(config: &TunnelConfig) -> Result<Option<Box<dyn Tunnel>>> {
    match config.provider.as_str() {
        "none" | "" => Ok(None),

        "cloudflare" => {
            let cf = config.cloudflare.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "tunnel.provider = \"cloudflare\" but [tunnel.cloudflare] section is missing"
                )
            })?;
            Ok(Some(Box::new(CloudflareTunnel::new(cf.token.clone()))))
        }

        "tailscale" => {
            let ts = config.tailscale.as_ref().unwrap_or(&TailscaleTunnelConfig {
                funnel: false,
                hostname: None,
            });
            Ok(Some(Box::new(TailscaleTunnel::new(
                ts.funnel,
                ts.hostname.clone(),
            ))))
        }

        "ngrok" => {
            let ng = config.ngrok.as_ref().ok_or_else(|| {
                anyhow::anyhow!("tunnel.provider = \"ngrok\" but [tunnel.ngrok] section is missing")
            })?;
            Ok(Some(Box::new(NgrokTunnel::new(
                ng.auth_token.clone(),
                ng.domain.clone(),
            ))))
        }

        "custom" => {
            let cu = config.custom.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "tunnel.provider = \"custom\" but [tunnel.custom] section is missing"
                )
            })?;
            Ok(Some(Box::new(CustomTunnel::new(
                cu.start_command.clone(),
                cu.health_url.clone(),
                cu.url_pattern.clone(),
            ))))
        }

        other => bail!(
            "Unknown tunnel provider: \"{other}\". Valid: none, cloudflare, tailscale, ngrok, custom"
        ),
    }
}
