mod cloudflare;
mod custom;
mod factory;
mod ngrok;
mod none;
mod process;
mod tailscale;
mod traits;

#[cfg(test)]
mod tests;

pub use cloudflare::CloudflareTunnel;
pub use custom::CustomTunnel;
pub use factory::create_tunnel;
pub use ngrok::NgrokTunnel;
#[allow(unused_imports)]
pub use none::NoneTunnel;
pub(crate) use process::{SharedProcess, TunnelProcess, kill_shared, new_shared_process};
pub use tailscale::TailscaleTunnel;
pub use traits::Tunnel;
