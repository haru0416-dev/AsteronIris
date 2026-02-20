use async_trait::async_trait;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

/// Trait for embedding providers — convert text to vectors
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Provider name
    fn name(&self) -> &str;

    /// Embedding dimensions
    fn dimensions(&self) -> usize;

    /// Embed a batch of texts into vectors
    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;

    /// Embed a single text
    async fn embed_one(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut results = self.embed(&[text]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding result"))
    }
}

#[cfg(test)]
pub(crate) struct DeterministicEmbedding {
    dims: usize,
    seed: u64,
}

#[cfg(test)]
impl DeterministicEmbedding {
    pub(crate) fn new(dims: usize) -> Self {
        Self { dims, seed: 0 }
    }

    pub(crate) fn with_seed(dims: usize, seed: u64) -> Self {
        Self { dims, seed }
    }

    fn fnv1a64(seed: u64, bytes: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    fn splitmix64(mut x: u64) -> u64 {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    #[allow(clippy::cast_precision_loss)]
    fn u64_to_unit_f32(x: u64) -> f32 {
        const U24_MAX: f32 = ((1u32 << 24) - 1) as f32;
        let top_u24: u32 = (x >> 40) as u32;
        (top_u24 as f32 / U24_MAX) * 2.0 - 1.0
    }
}

#[cfg(test)]
#[async_trait]
impl EmbeddingProvider for DeterministicEmbedding {
    fn name(&self) -> &str {
        "deterministic_test"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for &t in texts {
            let base = Self::fnv1a64(self.seed, t.as_bytes());
            let mut v = Vec::with_capacity(self.dims);
            for i in 0..self.dims {
                let mixed = Self::splitmix64(base ^ (i as u64));
                v.push(Self::u64_to_unit_f32(mixed));
            }
            out.push(v);
        }
        Ok(out)
    }
}

// ── Noop provider (keyword-only fallback) ────────────────────

pub struct NoopEmbedding;

#[async_trait]
impl EmbeddingProvider for NoopEmbedding {
    fn name(&self) -> &str {
        "none"
    }

    fn dimensions(&self) -> usize {
        0
    }

    async fn embed(&self, _texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(Vec::new())
    }
}

// ── OpenAI-compatible embedding provider ─────────────────────

pub struct OpenAiEmbedding {
    client: reqwest::Client,
    cached_embeddings_url: String,
    cached_auth_header: String,
    model: String,
    dims: usize,
}

#[derive(Copy, Clone, Debug)]
struct CustomBaseUrlPolicy {
    allow_http: bool,
}

fn is_ssrf_blocked_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn is_ssrf_blocked_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }

    let seg0 = ip.segments()[0];
    let is_link_local = (seg0 & 0xffc0) == 0xfe80;
    let is_unique_local = (seg0 & 0xfe00) == 0xfc00;

    is_link_local || is_unique_local
}

fn is_ssrf_blocked_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    let host = host.trim_start_matches('[').trim_end_matches(']');

    if host.eq_ignore_ascii_case("metadata.google.internal") {
        return true;
    }

    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    let host_lc = host.to_ascii_lowercase();
    if let Ok(ip) = host_lc.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_ssrf_blocked_ipv4(v4),
            IpAddr::V6(v6) => is_ssrf_blocked_ipv6(v6),
        };
    }

    false
}

fn validate_custom_base_url(raw: &str, policy: CustomBaseUrlPolicy) -> anyhow::Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("custom embedding base URL is empty");
    }

    let url = reqwest::Url::parse(raw)
        .map_err(|_| anyhow::anyhow!("invalid custom embedding base URL"))?;

    match url.scheme() {
        "https" => {}
        "http" if policy.allow_http => {}
        "http" => anyhow::bail!("custom embedding base URL must use https"),
        _ => anyhow::bail!("custom embedding base URL must use http(s)"),
    }

    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("custom embedding base URL must not include userinfo");
    }

    if url.query().is_some() || url.fragment().is_some() {
        anyhow::bail!("custom embedding base URL must not include query or fragment");
    }

    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("custom embedding base URL missing host"))?;

    if is_ssrf_blocked_host(host) {
        anyhow::bail!("custom embedding base URL host is blocked");
    }

    Ok(url.as_str().trim_end_matches('/').to_string())
}

impl OpenAiEmbedding {
    pub fn new(base_url: &str, api_key: &str, model: &str, dims: usize) -> Self {
        let base = base_url.trim_end_matches('/');
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            cached_embeddings_url: format!("{base}/v1/embeddings"),
            cached_auth_header: format!("Bearer {api_key}"),
            model: model.to_string(),
            dims,
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbedding {
    fn name(&self) -> &str {
        "openai"
    }

    fn dimensions(&self) -> usize {
        self.dims
    }

    async fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });

        let resp = self
            .client
            .post(&self.cached_embeddings_url)
            .header("Authorization", &self.cached_auth_header)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Embedding HTTP request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            anyhow::bail!("Embedding API error {status}");
        }

        let json: serde_json::Value = resp.json().await?;
        let data = json
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response: missing 'data'"))?;

        let mut embeddings = Vec::with_capacity(data.len());
        for item in data {
            let embedding = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| anyhow::anyhow!("Invalid embedding item"))?;

            #[allow(clippy::cast_possible_truncation)]
            let vec: Vec<f32> = embedding
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            embeddings.push(vec);
        }

        Ok(embeddings)
    }
}

// ── Factory ──────────────────────────────────────────────────

pub fn create_embedding_provider(
    provider: &str,
    api_key: Option<&str>,
    model: &str,
    dims: usize,
) -> Box<dyn EmbeddingProvider> {
    match provider {
        "openai" => {
            let key = api_key.unwrap_or("");
            Box::new(OpenAiEmbedding::new(
                "https://api.openai.com",
                key,
                model,
                dims,
            ))
        }
        name if name.starts_with("custom:") => {
            let base_url = name.strip_prefix("custom:").unwrap_or("");
            let key = api_key.unwrap_or("");
            let policy = CustomBaseUrlPolicy {
                allow_http: cfg!(test),
            };

            match validate_custom_base_url(base_url, policy) {
                Ok(valid_base_url) => {
                    Box::new(OpenAiEmbedding::new(&valid_base_url, key, model, dims))
                }
                Err(_) => Box::new(NoopEmbedding),
            }
        }
        _ => Box::new(NoopEmbedding),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_name() {
        let p = NoopEmbedding;
        assert_eq!(p.name(), "none");
        assert_eq!(p.dimensions(), 0);
    }

    #[tokio::test]
    async fn noop_embed_returns_empty() {
        let p = NoopEmbedding;
        let result = p.embed(&["hello"]).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn factory_none() {
        let p = create_embedding_provider("none", None, "model", 1536);
        assert_eq!(p.name(), "none");
    }

    #[test]
    fn factory_openai() {
        let p = create_embedding_provider("openai", Some("key"), "text-embedding-3-small", 1536);
        assert_eq!(p.name(), "openai");
        assert_eq!(p.dimensions(), 1536);
    }

    #[test]
    fn factory_custom_url() {
        let p = create_embedding_provider("custom:https://example.com", None, "model", 768);
        assert_eq!(p.name(), "openai"); // uses OpenAiEmbedding internally
        assert_eq!(p.dimensions(), 768);
    }

    // ── Edge cases ───────────────────────────────────────────────

    #[tokio::test]
    async fn noop_embed_one_returns_error() {
        let p = NoopEmbedding;
        // embed returns empty vec → pop() returns None → error
        let result = p.embed_one("hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn noop_embed_empty_batch() {
        let p = NoopEmbedding;
        let result = p.embed(&[]).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn noop_embed_multiple_texts() {
        let p = NoopEmbedding;
        let result = p.embed(&["a", "b", "c"]).await.unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn factory_empty_string_returns_noop() {
        let p = create_embedding_provider("", None, "model", 1536);
        assert_eq!(p.name(), "none");
    }

    #[test]
    fn factory_unknown_provider_returns_noop() {
        let p = create_embedding_provider("cohere", None, "model", 1536);
        assert_eq!(p.name(), "none");
    }

    #[test]
    fn factory_custom_empty_url() {
        let p = create_embedding_provider("custom:", None, "model", 768);
        assert_eq!(p.name(), "none");
    }

    #[test]
    fn custom_url_allows_http_only_in_tests() {
        let p = create_embedding_provider("custom:http://example.com", None, "model", 1);
        assert_eq!(p.name(), "openai");
    }

    #[test]
    fn custom_url_blocks_localhost() {
        let p = create_embedding_provider("custom:https://localhost:1234", None, "model", 1);
        assert_eq!(p.name(), "none");
    }

    #[test]
    fn custom_url_blocks_private_ipv4_ranges() {
        for u in [
            "https://10.0.0.1",
            "https://172.16.0.1",
            "https://192.168.1.1",
            "https://169.254.0.1",
            "https://127.0.0.1",
        ] {
            let out = validate_custom_base_url(u, CustomBaseUrlPolicy { allow_http: true });
            assert!(out.is_err(), "expected blocked URL: {u}");
        }
    }

    #[test]
    fn custom_url_blocks_ipv6_loopback_and_link_local() {
        for u in ["https://[::1]", "https://[fe80::1]"] {
            let out = validate_custom_base_url(u, CustomBaseUrlPolicy { allow_http: true });
            assert!(out.is_err(), "expected blocked URL: {u}");
        }
    }

    #[test]
    fn custom_url_blocks_metadata_host() {
        let out = validate_custom_base_url(
            "https://metadata.google.internal",
            CustomBaseUrlPolicy { allow_http: true },
        );
        assert!(out.is_err());
    }

    #[test]
    fn custom_url_rejects_invalid_url() {
        let out = validate_custom_base_url("not a url", CustomBaseUrlPolicy { allow_http: true });
        assert!(out.is_err());
    }

    #[test]
    fn factory_openai_no_api_key() {
        let p = create_embedding_provider("openai", None, "text-embedding-3-small", 1536);
        assert_eq!(p.name(), "openai");
        assert_eq!(p.dimensions(), 1536);
    }

    #[test]
    fn openai_trailing_slash_stripped() {
        let p = OpenAiEmbedding::new("https://api.openai.com/", "key", "model", 1536);
        assert_eq!(
            p.cached_embeddings_url,
            "https://api.openai.com/v1/embeddings"
        );
    }

    #[test]
    fn openai_dimensions_custom() {
        let p = OpenAiEmbedding::new("http://localhost", "k", "m", 384);
        assert_eq!(p.dimensions(), 384);
    }

    #[tokio::test]
    async fn deterministic_embedder_is_stable_and_dimensional() {
        let p = DeterministicEmbedding::with_seed(8, 42);

        let a1 = p.embed_one("hello").await.unwrap();
        let a2 = p.embed_one("hello").await.unwrap();
        let b = p.embed_one("world").await.unwrap();

        assert_eq!(a1.len(), 8);
        assert_eq!(a2.len(), 8);
        assert_eq!(b.len(), 8);
        assert_eq!(a1, a2);
        assert_ne!(a1, b);

        for x in &a1 {
            assert!(x.is_finite());
            assert!(*x >= -1.0 && *x <= 1.0);
        }

        let batch = p.embed(&["a", "b"]).await.unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].len(), 8);
        assert_eq!(batch[1].len(), 8);
    }
}
