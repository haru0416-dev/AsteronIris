use reqwest::Client;
use std::time::Duration;

pub fn build_provider_client() -> Client {
    build_provider_client_with_timeout(120)
}

pub fn build_provider_client_with_timeout(timeout_secs: u64) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| Client::new())
}
