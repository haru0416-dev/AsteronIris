use arc_swap::ArcSwap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::cooldown::CooldownTracker;
use super::factory;
use super::traits::Provider;
use crate::config::Config;

/// Central manager for LLM provider lifecycle.
///
/// Holds a hot-swappable [`Config`] (via [`ArcSwap`]) and a per-provider
/// [`CooldownTracker`]. Callers obtain a `Box<dyn Provider>` through
/// [`get_provider`](Self::get_provider) which applies the full decorator
/// chain (OAuth recovery, resilient retry/fallback).
pub struct LlmManager {
    config: Arc<ArcSwap<Config>>,
    cooldown: Mutex<CooldownTracker>,
}

impl LlmManager {
    pub fn new(config: Arc<ArcSwap<Config>>) -> Self {
        Self {
            config,
            cooldown: Mutex::new(CooldownTracker::new()),
        }
    }

    /// Build a provider from the current config, wrapped in the full
    /// resilient + OAuth-recovery decorator chain.
    ///
    /// Returns an error if the configured provider name is unknown or if
    /// the provider is currently in a cooldown window.
    pub fn get_provider(&self) -> anyhow::Result<Box<dyn Provider>> {
        let cfg = self.config.load();

        let provider_name = cfg.default_provider.as_deref().unwrap_or("anthropic");

        // Check cooldown before creating the provider.
        {
            let tracker = self.cooldown.lock().expect("cooldown lock poisoned");
            if tracker.is_cooling_down(provider_name) {
                let remaining = tracker.remaining(provider_name).unwrap_or(Duration::ZERO);
                anyhow::bail!(
                    "Provider \"{provider_name}\" is rate-limited (cooldown remaining: {remaining:.0?})"
                );
            }
        }

        let api_key = cfg.api_key.as_deref();

        factory::create_resilient_provider_with_oauth_recovery(
            &cfg,
            provider_name,
            &cfg.reliability,
            |name| factory::resolve_api_key(name, api_key),
        )
    }

    /// Create a provider for a specific named backend, bypassing config
    /// defaults. Useful for explicit fallback selection.
    pub fn get_named_provider(
        &self,
        name: &str,
        api_key: Option<&str>,
    ) -> anyhow::Result<Box<dyn Provider>> {
        {
            let tracker = self.cooldown.lock().expect("cooldown lock poisoned");
            if tracker.is_cooling_down(name) {
                let remaining = tracker.remaining(name).unwrap_or(Duration::ZERO);
                anyhow::bail!(
                    "Provider \"{name}\" is rate-limited (cooldown remaining: {remaining:.0?})"
                );
            }
        }

        factory::create_provider(name, api_key)
    }

    /// Returns `true` if the named provider is not currently in a cooldown
    /// window.
    pub fn is_provider_available(&self, name: &str) -> bool {
        let tracker = self.cooldown.lock().expect("cooldown lock poisoned");
        !tracker.is_cooling_down(name)
    }

    /// Record a rate-limit cooldown for the named provider.
    pub fn set_cooldown(&self, provider: &str, duration: Duration) {
        let mut tracker = self.cooldown.lock().expect("cooldown lock poisoned");
        tracker.set_cooldown(provider, duration);
    }

    /// Clear the cooldown for a specific provider, making it immediately
    /// available again.
    pub fn clear_cooldown(&self, provider: &str) {
        let mut tracker = self.cooldown.lock().expect("cooldown lock poisoned");
        tracker.clear(provider);
    }

    /// Hot-swap the configuration. Subsequent calls to [`get_provider`](Self::get_provider)
    /// will use the new config.
    pub fn update_config(&self, new_config: Arc<Config>) {
        self.config.store(new_config);
    }

    /// Return a snapshot of the current configuration.
    pub fn current_config(&self) -> arc_swap::Guard<Arc<Config>> {
        self.config.load()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Arc<ArcSwap<Config>> {
        let cfg = Config {
            default_provider: Some("anthropic".to_string()),
            ..Config::default()
        };
        Arc::new(ArcSwap::new(Arc::new(cfg)))
    }

    #[test]
    fn new_manager_has_no_cooldowns() {
        let mgr = LlmManager::new(test_config());
        assert!(mgr.is_provider_available("anthropic"));
        assert!(mgr.is_provider_available("openai"));
    }

    #[test]
    fn set_cooldown_makes_provider_unavailable() {
        let mgr = LlmManager::new(test_config());
        mgr.set_cooldown("anthropic", Duration::from_secs(60));
        assert!(!mgr.is_provider_available("anthropic"));
        // Other providers are unaffected.
        assert!(mgr.is_provider_available("openai"));
    }

    #[test]
    fn clear_cooldown_restores_availability() {
        let mgr = LlmManager::new(test_config());
        mgr.set_cooldown("anthropic", Duration::from_secs(60));
        assert!(!mgr.is_provider_available("anthropic"));
        mgr.clear_cooldown("anthropic");
        assert!(mgr.is_provider_available("anthropic"));
    }

    #[test]
    fn get_provider_fails_during_cooldown() {
        let mgr = LlmManager::new(test_config());
        mgr.set_cooldown("anthropic", Duration::from_secs(60));
        let msg = mgr
            .get_provider()
            .err()
            .expect("should fail during cooldown")
            .to_string();
        assert!(msg.contains("rate-limited"));
    }

    #[test]
    fn get_provider_succeeds_without_cooldown() {
        let mgr = LlmManager::new(test_config());
        // Should succeed — creates an Anthropic provider (no API key
        // needed for construction).
        let result = mgr.get_provider();
        assert!(result.is_ok());
    }

    #[test]
    fn update_config_changes_provider() {
        let config = test_config();
        let mgr = LlmManager::new(Arc::clone(&config));

        // Switch to gemini
        let new_cfg = Config {
            default_provider: Some("gemini".to_string()),
            ..Config::default()
        };
        mgr.update_config(Arc::new(new_cfg));

        let loaded = mgr.current_config();
        assert_eq!(loaded.default_provider.as_deref(), Some("gemini"));
    }

    #[test]
    fn get_named_provider_respects_cooldown() {
        let mgr = LlmManager::new(test_config());
        mgr.set_cooldown("anthropic", Duration::from_secs(60));
        let msg = mgr
            .get_named_provider("anthropic", None)
            .err()
            .expect("should fail during cooldown")
            .to_string();
        assert!(msg.contains("rate-limited"));
    }

    #[test]
    fn get_named_provider_succeeds_without_cooldown() {
        let mgr = LlmManager::new(test_config());
        let result = mgr.get_named_provider("anthropic", None);
        assert!(result.is_ok());
    }

    #[test]
    fn expired_cooldown_allows_provider_creation() {
        let mgr = LlmManager::new(test_config());
        // Zero-duration cooldown expires immediately.
        mgr.set_cooldown("anthropic", Duration::ZERO);
        assert!(mgr.is_provider_available("anthropic"));
        assert!(mgr.get_provider().is_ok());
    }
}
