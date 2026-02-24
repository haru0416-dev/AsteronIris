use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Tracks per-provider rate-limit cooldown windows.
///
/// When a provider returns a rate-limit response, callers record a cooldown
/// via [`set_cooldown`](Self::set_cooldown). Subsequent calls to
/// [`is_cooling_down`](Self::is_cooling_down) will return `true` until the
/// window expires.
pub struct CooldownTracker {
    cooldowns: HashMap<String, Instant>,
}

impl CooldownTracker {
    pub fn new() -> Self {
        Self {
            cooldowns: HashMap::new(),
        }
    }

    /// Record a cooldown for `provider` that expires after `duration`.
    pub fn set_cooldown(&mut self, provider: &str, duration: Duration) {
        let expires_at = Instant::now() + duration;
        self.cooldowns.insert(provider.to_string(), expires_at);
    }

    /// Returns `true` if `provider` is still within a cooldown window.
    pub fn is_cooling_down(&self, provider: &str) -> bool {
        self.cooldowns
            .get(provider)
            .is_some_and(|expires_at| Instant::now() < *expires_at)
    }

    /// Returns the remaining cooldown duration for `provider`, or `None` if
    /// the provider is not cooling down (or the cooldown has expired).
    pub fn remaining(&self, provider: &str) -> Option<Duration> {
        let expires_at = self.cooldowns.get(provider)?;
        let now = Instant::now();
        if now < *expires_at {
            Some(*expires_at - now)
        } else {
            None
        }
    }

    /// Remove the cooldown entry for a specific provider.
    pub fn clear(&mut self, provider: &str) {
        self.cooldowns.remove(provider);
    }

    /// Remove all expired cooldown entries.
    pub fn clear_expired(&mut self) {
        let now = Instant::now();
        self.cooldowns.retain(|_, expires_at| now < *expires_at);
    }
}

impl Default for CooldownTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::CooldownTracker;
    use std::time::Duration;

    #[test]
    fn new_tracker_has_no_cooldowns() {
        let tracker = CooldownTracker::new();
        assert!(!tracker.is_cooling_down("openai"));
        assert!(tracker.remaining("openai").is_none());
    }

    #[test]
    fn set_cooldown_makes_provider_cool_down() {
        let mut tracker = CooldownTracker::new();
        tracker.set_cooldown("openai", Duration::from_secs(60));
        assert!(tracker.is_cooling_down("openai"));
        assert!(tracker.remaining("openai").is_some());
    }

    #[test]
    fn different_providers_are_independent() {
        let mut tracker = CooldownTracker::new();
        tracker.set_cooldown("openai", Duration::from_secs(60));
        assert!(tracker.is_cooling_down("openai"));
        assert!(!tracker.is_cooling_down("anthropic"));
    }

    #[test]
    fn clear_removes_specific_provider() {
        let mut tracker = CooldownTracker::new();
        tracker.set_cooldown("openai", Duration::from_secs(60));
        tracker.set_cooldown("anthropic", Duration::from_secs(60));
        tracker.clear("openai");
        assert!(!tracker.is_cooling_down("openai"));
        assert!(tracker.is_cooling_down("anthropic"));
    }

    #[test]
    fn expired_cooldown_is_not_active() {
        let mut tracker = CooldownTracker::new();
        // Zero-duration cooldown expires immediately.
        tracker.set_cooldown("openai", Duration::ZERO);
        assert!(!tracker.is_cooling_down("openai"));
        assert!(tracker.remaining("openai").is_none());
    }

    #[test]
    fn clear_expired_removes_stale_entries() {
        let mut tracker = CooldownTracker::new();
        tracker.set_cooldown("expired", Duration::ZERO);
        tracker.set_cooldown("active", Duration::from_secs(60));
        tracker.clear_expired();
        // The expired entry should be gone but active remains.
        assert!(!tracker.is_cooling_down("expired"));
        assert!(tracker.is_cooling_down("active"));
    }

    #[test]
    fn remaining_returns_positive_duration_for_active_cooldown() {
        let mut tracker = CooldownTracker::new();
        tracker.set_cooldown("openai", Duration::from_secs(300));
        let remaining = tracker.remaining("openai").unwrap();
        // Should be close to 300s (allow some slack for test execution).
        assert!(remaining.as_secs() >= 299);
    }

    #[test]
    fn default_impl_creates_empty_tracker() {
        let tracker = CooldownTracker::default();
        assert!(!tracker.is_cooling_down("any"));
    }
}
