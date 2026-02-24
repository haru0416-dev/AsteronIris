use super::streaming::ProviderStream;
use super::traits::{Provider, ProviderCapabilities};
use super::types::{ProviderMessage, ProviderResponse};
use crate::tools::ToolSpec;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Check if an error is non-retryable (client errors that won't resolve with retries).
fn is_non_retryable(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    if is_quota_exhausted(&msg) {
        return true;
    }

    // Check for reqwest status errors (returned by .error_for_status())
    if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>()
        && let Some(status) = reqwest_err.status()
    {
        let code = status.as_u16();
        // 4xx client errors are non-retryable, except:
        // - 429 Too Many Requests (rate limiting, transient)
        // - 408 Request Timeout (transient)
        return status.is_client_error() && code != 429 && code != 408;
    }
    // String fallback: scan for any 4xx status code in error message
    for word in msg.split(|c: char| !c.is_ascii_digit()) {
        if let Ok(code) = word.parse::<u16>()
            && (400..500).contains(&code)
        {
            return code != 429 && code != 408;
        }
    }
    false
}

fn is_quota_exhausted(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("insufficient_quota")
        || lower.contains("exceeded your current quota")
        || lower.contains("billing")
}

/// Provider wrapper with retry + fallback behavior.
pub struct ReliableProvider {
    providers: Vec<(String, Box<dyn Provider>)>,
    max_retries: u32,
    base_backoff_ms: u64,
}

impl ReliableProvider {
    pub fn new(
        providers: Vec<(String, Box<dyn Provider>)>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        Self {
            providers,
            max_retries,
            base_backoff_ms: base_backoff_ms.max(50),
        }
    }
}

impl Provider for ReliableProvider {
    fn name(&self) -> &str {
        self.providers
            .first()
            .map_or("reliable", |(name, _)| name.as_str())
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.providers
            .first()
            .map_or_else(ProviderCapabilities::default, |(_, p)| p.capabilities())
    }

    fn warmup(&self) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + '_>> {
        Box::pin(async move {
            for (name, provider) in &self.providers {
                tracing::info!(
                    provider = name.as_str(),
                    "Warming up provider connection pool"
                );
                if let Err(e) = provider.warmup().await {
                    tracing::warn!(provider = name.as_str(), "Warmup failed (non-fatal): {e}");
                }
            }
            Ok(())
        })
    }

    fn chat_with_system<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let mut failures = Vec::new();

            for (provider_name, provider) in &self.providers {
                let mut backoff_ms = self.base_backoff_ms;

                for attempt in 0..=self.max_retries {
                    match provider
                        .chat_with_system(system_prompt, message, model, temperature)
                        .await
                    {
                        Ok(resp) => {
                            if attempt > 0 {
                                tracing::info!(
                                    provider = provider_name.as_str(),
                                    attempt,
                                    "Provider recovered after retries"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            let non_retryable = is_non_retryable(&e);
                            failures.push(format!(
                                "{provider_name} attempt {}/{}: {e}",
                                attempt + 1,
                                self.max_retries + 1
                            ));

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    "Non-retryable error, switching provider"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    attempt = attempt + 1,
                                    max_retries = self.max_retries,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name.as_str(),
                    "Switching to fallback provider"
                );
            }

            anyhow::bail!("All providers failed. Attempts:\n{}", failures.join("\n"))
        })
    }

    fn chat_with_system_full<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let mut failures = Vec::new();

            for (provider_name, provider) in &self.providers {
                let mut backoff_ms = self.base_backoff_ms;

                for attempt in 0..=self.max_retries {
                    match provider
                        .chat_with_system_full(system_prompt, message, model, temperature)
                        .await
                    {
                        Ok(resp) => {
                            if attempt > 0 {
                                tracing::info!(
                                    provider = provider_name.as_str(),
                                    attempt,
                                    "Provider recovered after retries"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            let non_retryable = is_non_retryable(&e);
                            failures.push(format!(
                                "{provider_name} attempt {}/{}: {e}",
                                attempt + 1,
                                self.max_retries + 1
                            ));

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    "Non-retryable error, switching provider"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    attempt = attempt + 1,
                                    max_retries = self.max_retries,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name.as_str(),
                    "Switching to fallback provider"
                );
            }

            anyhow::bail!("All providers failed. Attempts:\n{}", failures.join("\n"))
        })
    }

    fn chat_with_tools<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        messages: &'a [ProviderMessage],
        tools: &'a [ToolSpec],
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let mut failures = Vec::new();

            for (provider_name, provider) in &self.providers {
                let mut backoff_ms = self.base_backoff_ms;

                for attempt in 0..=self.max_retries {
                    match provider
                        .chat_with_tools(system_prompt, messages, tools, model, temperature)
                        .await
                    {
                        Ok(resp) => {
                            if attempt > 0 {
                                tracing::info!(
                                    provider = provider_name.as_str(),
                                    attempt,
                                    "Provider recovered after retries"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            let non_retryable = is_non_retryable(&e);
                            failures.push(format!(
                                "{provider_name} attempt {}/{}: {e}",
                                attempt + 1,
                                self.max_retries + 1
                            ));

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    "Non-retryable error, switching provider"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    attempt = attempt + 1,
                                    max_retries = self.max_retries,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name.as_str(),
                    "Switching to fallback provider"
                );
            }

            anyhow::bail!("All providers failed. Attempts:\n{}", failures.join("\n"))
        })
    }

    fn chat_with_tools_stream<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        messages: &'a [ProviderMessage],
        tools: &'a [ToolSpec],
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ProviderStream>> + Send + 'a>> {
        Box::pin(async move {
            let mut failures = Vec::new();

            for (provider_name, provider) in &self.providers {
                let mut backoff_ms = self.base_backoff_ms;

                for attempt in 0..=self.max_retries {
                    match provider
                        .chat_with_tools_stream(system_prompt, messages, tools, model, temperature)
                        .await
                    {
                        Ok(resp) => {
                            if attempt > 0 {
                                tracing::info!(
                                    provider = provider_name.as_str(),
                                    attempt,
                                    "Provider recovered after retries"
                                );
                            }
                            return Ok(resp);
                        }
                        Err(e) => {
                            let non_retryable = is_non_retryable(&e);
                            failures.push(format!(
                                "{provider_name} attempt {}/{}: {e}",
                                attempt + 1,
                                self.max_retries + 1
                            ));

                            if non_retryable {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    "Non-retryable error, switching provider"
                                );
                                break;
                            }

                            if attempt < self.max_retries {
                                tracing::warn!(
                                    provider = provider_name.as_str(),
                                    attempt = attempt + 1,
                                    max_retries = self.max_retries,
                                    "Provider call failed, retrying"
                                );
                                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                                backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                            }
                        }
                    }
                }

                tracing::warn!(
                    provider = provider_name.as_str(),
                    "Switching to fallback provider"
                );
            }

            anyhow::bail!("All providers failed. Attempts:\n{}", failures.join("\n"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockProvider {
        calls: Arc<AtomicUsize>,
        fail_until_attempt: usize,
        response: &'static str,
        error: &'static str,
    }

    impl Provider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn chat_with_system<'a>(
            &'a self,
            _system_prompt: Option<&'a str>,
            _message: &'a str,
            _model: &'a str,
            _temperature: f64,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send + 'a>> {
            Box::pin(async move {
                let attempt = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt <= self.fail_until_attempt {
                    anyhow::bail!(self.error);
                }
                Ok(self.response.to_string())
            })
        }
    }

    #[tokio::test]
    async fn succeeds_without_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = ReliableProvider::new(
            vec![(
                "primary".into(),
                Box::new(MockProvider {
                    calls: Arc::clone(&calls),
                    fail_until_attempt: 0,
                    response: "ok",
                    error: "boom",
                }),
            )],
            2,
            1,
        );

        let result = provider.chat("hello", "test", 0.0).await.unwrap();
        assert_eq!(result, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_then_recovers() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = ReliableProvider::new(
            vec![(
                "primary".into(),
                Box::new(MockProvider {
                    calls: Arc::clone(&calls),
                    fail_until_attempt: 1,
                    response: "recovered",
                    error: "temporary",
                }),
            )],
            2,
            1,
        );

        let result = provider.chat("hello", "test", 0.0).await.unwrap();
        assert_eq!(result, "recovered");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn falls_back_after_retries_exhausted() {
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));

        let provider = ReliableProvider::new(
            vec![
                (
                    "primary".into(),
                    Box::new(MockProvider {
                        calls: Arc::clone(&primary_calls),
                        fail_until_attempt: usize::MAX,
                        response: "never",
                        error: "primary down",
                    }),
                ),
                (
                    "fallback".into(),
                    Box::new(MockProvider {
                        calls: Arc::clone(&fallback_calls),
                        fail_until_attempt: 0,
                        response: "from fallback",
                        error: "fallback down",
                    }),
                ),
            ],
            1,
            1,
        );

        let result = provider.chat("hello", "test", 0.0).await.unwrap();
        assert_eq!(result, "from fallback");
        assert_eq!(primary_calls.load(Ordering::SeqCst), 2);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn returns_aggregated_error_when_all_providers_fail() {
        let provider = ReliableProvider::new(
            vec![
                (
                    "p1".into(),
                    Box::new(MockProvider {
                        calls: Arc::new(AtomicUsize::new(0)),
                        fail_until_attempt: usize::MAX,
                        response: "never",
                        error: "p1 error",
                    }),
                ),
                (
                    "p2".into(),
                    Box::new(MockProvider {
                        calls: Arc::new(AtomicUsize::new(0)),
                        fail_until_attempt: usize::MAX,
                        response: "never",
                        error: "p2 error",
                    }),
                ),
            ],
            0,
            1,
        );

        let err = provider
            .chat("hello", "test", 0.0)
            .await
            .expect_err("all providers should fail");
        let msg = err.to_string();
        assert!(msg.contains("All providers failed"));
        assert!(msg.contains("p1 attempt 1/1"));
        assert!(msg.contains("p2 attempt 1/1"));
    }

    #[test]
    fn non_retryable_detects_common_patterns() {
        // Non-retryable 4xx errors
        assert!(is_non_retryable(&anyhow::anyhow!("400 Bad Request")));
        assert!(is_non_retryable(&anyhow::anyhow!("401 Unauthorized")));
        assert!(is_non_retryable(&anyhow::anyhow!("403 Forbidden")));
        assert!(is_non_retryable(&anyhow::anyhow!("404 Not Found")));
        assert!(is_non_retryable(&anyhow::anyhow!(
            "API error with 400 Bad Request"
        )));
        // Retryable: 429 Too Many Requests
        assert!(!is_non_retryable(&anyhow::anyhow!("429 Too Many Requests")));
        // Retryable: 408 Request Timeout
        assert!(!is_non_retryable(&anyhow::anyhow!("408 Request Timeout")));
        // Retryable: 5xx server errors
        assert!(!is_non_retryable(&anyhow::anyhow!(
            "500 Internal Server Error"
        )));
        assert!(!is_non_retryable(&anyhow::anyhow!("502 Bad Gateway")));
        // Retryable: transient errors
        assert!(!is_non_retryable(&anyhow::anyhow!("timeout")));
        assert!(!is_non_retryable(&anyhow::anyhow!("connection reset")));

        assert!(is_non_retryable(&anyhow::anyhow!(
            "{}",
            "OpenAI API error (429 Too Many Requests): {\"error\":{\"message\":\"You exceeded your current quota\",\"type\":\"insufficient_quota\"}}"
        )));
    }

    #[tokio::test]
    async fn skips_retries_on_non_retryable_error() {
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));

        let provider = ReliableProvider::new(
            vec![
                (
                    "primary".into(),
                    Box::new(MockProvider {
                        calls: Arc::clone(&primary_calls),
                        fail_until_attempt: usize::MAX,
                        response: "never",
                        error: "401 Unauthorized",
                    }),
                ),
                (
                    "fallback".into(),
                    Box::new(MockProvider {
                        calls: Arc::clone(&fallback_calls),
                        fail_until_attempt: 0,
                        response: "from fallback",
                        error: "fallback err",
                    }),
                ),
            ],
            3, // 3 retries allowed, but should skip them
            1,
        );

        let result = provider.chat("hello", "test", 0.0).await.unwrap();
        assert_eq!(result, "from fallback");
        // Primary should have been called only once (no retries)
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);
    }
}
