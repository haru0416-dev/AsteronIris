use anyhow::Result;

/// Agnostic tunnel abstraction — bring your own tunnel provider.
///
/// Implementations wrap an external tunnel binary (cloudflared, tailscale,
/// ngrok, etc.) or a custom command. The gateway calls `start()` after
/// binding its local port and `stop()` on shutdown.
#[async_trait::async_trait]
pub trait Tunnel: Send + Sync {
    /// Human-readable provider name (e.g. "cloudflare", "tailscale")
    fn name(&self) -> &str;

    /// Start the tunnel, exposing `local_host:local_port` externally.
    /// Returns the public URL on success.
    async fn start(&self, local_host: &str, local_port: u16) -> Result<String>;

    /// Stop the tunnel process gracefully.
    async fn stop(&self) -> Result<()>;

    /// Check if the tunnel is still alive.
    async fn health_check(&self) -> bool;

    /// Return the public URL if the tunnel is running.
    fn public_url(&self) -> Option<String>;
}
