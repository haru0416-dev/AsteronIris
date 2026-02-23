use super::{Provider, ProviderResponse, sanitize_api_error};
use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

type RecoverFn = dyn Fn(&str) -> Result<bool> + Send + Sync;
type RebuildFn = dyn Fn(&str) -> Result<Arc<dyn Provider>> + Send + Sync;

struct RecoveryState {
    last_failed_at: Option<Instant>,
}

pub struct OAuthRecoveryProvider {
    provider_name: String,
    inner: RwLock<Arc<dyn Provider>>,
    recover: Arc<RecoverFn>,
    rebuild: Arc<RebuildFn>,
    state: Mutex<RecoveryState>,
    cooldown: Duration,
}

impl OAuthRecoveryProvider {
    pub fn new(
        provider_name: &str,
        inner: Arc<dyn Provider>,
        recover: Arc<RecoverFn>,
        rebuild: Arc<RebuildFn>,
    ) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            inner: RwLock::new(inner),
            recover,
            rebuild,
            state: Mutex::new(RecoveryState {
                last_failed_at: None,
            }),
            cooldown: Duration::from_secs(60),
        }
    }

    #[cfg(test)]
    fn with_cooldown(
        provider_name: &str,
        inner: Arc<dyn Provider>,
        recover: Arc<RecoverFn>,
        rebuild: Arc<RebuildFn>,
        cooldown: Duration,
    ) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            inner: RwLock::new(inner),
            recover,
            rebuild,
            state: Mutex::new(RecoveryState {
                last_failed_at: None,
            }),
            cooldown,
        }
    }

    fn is_auth_error(err: &anyhow::Error) -> bool {
        if let Some(reqwest_err) = err.downcast_ref::<reqwest::Error>()
            && let Some(status) = reqwest_err.status()
        {
            return status.as_u16() == 401 || status.as_u16() == 403;
        }

        let msg = err.to_string().to_ascii_lowercase();
        msg.contains("401")
            || msg.contains("403")
            || msg.contains("unauthorized")
            || msg.contains("authentication")
            || msg.contains("invalid api key")
            || msg.contains("invalid token")
            || msg.contains("token expired")
    }

    async fn attempt_recovery(&self) -> Result<bool> {
        {
            let state = self.state.lock().await;
            if state
                .last_failed_at
                .is_some_and(|failed_at| failed_at.elapsed() < self.cooldown)
            {
                return Ok(false);
            }
        }

        let provider_name = self.provider_name.clone();
        let recover = Arc::clone(&self.recover);
        let recovered = tokio::task::spawn_blocking(move || (recover)(&provider_name)).await??;
        if !recovered {
            let mut state = self.state.lock().await;
            state.last_failed_at = Some(Instant::now());
            return Ok(false);
        }

        let provider_name = self.provider_name.clone();
        let rebuild_fn = Arc::clone(&self.rebuild);
        let rebuilt_provider =
            tokio::task::spawn_blocking(move || (rebuild_fn)(&provider_name)).await??;
        *self.inner.write().await = rebuilt_provider;

        let mut state = self.state.lock().await;
        state.last_failed_at = None;
        Ok(true)
    }
}

impl Provider for OAuthRecoveryProvider {
    fn warmup(&self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let provider = self.inner.read().await.clone();
            provider.warmup().await
        })
    }

    fn chat_with_system<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let provider = self.inner.read().await.clone();
            let first_attempt = provider
                .chat_with_system(system_prompt, message, model, temperature)
                .await;

            let Err(first_error) = first_attempt else {
                return first_attempt;
            };

            if !Self::is_auth_error(&first_error) {
                return Err(first_error);
            }

            match self.attempt_recovery().await {
                Ok(true) => {
                    let provider = self.inner.read().await.clone();
                    provider
                        .chat_with_system(system_prompt, message, model, temperature)
                        .await
                }
                Ok(false) => Err(first_error),
                Err(recovery_error) => {
                    tracing::warn!(
                        provider = %self.provider_name,
                        "OAuth recovery failed: {}",
                        sanitize_api_error(&recovery_error.to_string())
                    );
                    Err(first_error)
                }
            }
        })
    }

    fn chat_with_system_full<'a>(
        &'a self,
        system_prompt: Option<&'a str>,
        message: &'a str,
        model: &'a str,
        temperature: f64,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse>> + Send + 'a>> {
        Box::pin(async move {
            let provider = self.inner.read().await.clone();
            let first_attempt = provider
                .chat_with_system_full(system_prompt, message, model, temperature)
                .await;

            let Err(first_error) = first_attempt else {
                return first_attempt;
            };

            if !Self::is_auth_error(&first_error) {
                return Err(first_error);
            }

            match self.attempt_recovery().await {
                Ok(true) => {
                    let provider = self.inner.read().await.clone();
                    provider
                        .chat_with_system_full(system_prompt, message, model, temperature)
                        .await
                }
                Ok(false) => Err(first_error),
                Err(recovery_error) => {
                    tracing::warn!(
                        provider = %self.provider_name,
                        "OAuth recovery failed: {}",
                        sanitize_api_error(&recovery_error.to_string())
                    );
                    Err(first_error)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::bail;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailProvider;

    impl Provider for FailProvider {
        fn chat_with_system<'a>(
            &'a self,
            _system_prompt: Option<&'a str>,
            _message: &'a str,
            _model: &'a str,
            _temperature: f64,
        ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
            Box::pin(async move { bail!("401 unauthorized") })
        }
    }

    struct OkProvider;

    impl Provider for OkProvider {
        fn chat_with_system<'a>(
            &'a self,
            _system_prompt: Option<&'a str>,
            _message: &'a str,
            _model: &'a str,
            _temperature: f64,
        ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
            Box::pin(async move { Ok("ok".to_string()) })
        }
    }

    #[tokio::test]
    async fn retries_once_after_recovery_and_rebuild() {
        let recover_calls = Arc::new(AtomicUsize::new(0));
        let rebuild_calls = Arc::new(AtomicUsize::new(0));

        let recover = {
            let recover_calls = Arc::clone(&recover_calls);
            Arc::new(move |_provider: &str| {
                recover_calls.fetch_add(1, Ordering::SeqCst);
                Ok(true)
            })
        };

        let rebuild = {
            let rebuild_calls = Arc::clone(&rebuild_calls);
            Arc::new(move |_provider: &str| {
                rebuild_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(OkProvider) as Arc<dyn Provider>)
            })
        };

        let provider =
            OAuthRecoveryProvider::new("openai", Arc::new(FailProvider), recover, rebuild);

        let result = provider.chat("hello", "gpt-test", 0.0).await.unwrap();
        assert_eq!(result, "ok");
        assert_eq!(recover_calls.load(Ordering::SeqCst), 1);
        assert_eq!(rebuild_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cooldown_skips_repeat_recovery_after_failure() {
        let recover_calls = Arc::new(AtomicUsize::new(0));
        let rebuild_calls = Arc::new(AtomicUsize::new(0));

        let recover = {
            let recover_calls = Arc::clone(&recover_calls);
            Arc::new(move |_provider: &str| {
                recover_calls.fetch_add(1, Ordering::SeqCst);
                Ok(false)
            })
        };

        let rebuild = {
            let rebuild_calls = Arc::clone(&rebuild_calls);
            Arc::new(move |_provider: &str| {
                rebuild_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(OkProvider) as Arc<dyn Provider>)
            })
        };

        let provider = OAuthRecoveryProvider::with_cooldown(
            "openai",
            Arc::new(FailProvider),
            recover,
            rebuild,
            Duration::from_secs(60),
        );

        assert!(provider.chat("hello", "gpt-test", 0.0).await.is_err());
        assert!(provider.chat("hello", "gpt-test", 0.0).await.is_err());
        assert_eq!(recover_calls.load(Ordering::SeqCst), 1);
        assert_eq!(rebuild_calls.load(Ordering::SeqCst), 0);
    }
}
