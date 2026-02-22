//! Replay protection for webhook endpoints.
//!
//! Tracks SHA-256 hashes of recent request bodies within a TTL window.
//! In-memory only — intentionally resets on restart.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_TTL_SECS: u64 = 300;
const MAX_ENTRIES: usize = 10_000;

pub struct ReplayGuard {
    seen: Mutex<HashMap<String, Instant>>,
    ttl: Duration,
}

impl ReplayGuard {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(DEFAULT_TTL_SECS),
        }
    }

    /// Returns `true` if new (process), `false` if replay (reject).
    pub fn check_and_record(&self, body: &[u8]) -> bool {
        let nonce = hex::encode(Sha256::digest(body));
        let now = Instant::now();
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.len() > MAX_ENTRIES {
            seen.retain(|_, ts| now.duration_since(*ts) < self.ttl);
        }
        if let Some(ts) = seen.get(&nonce)
            && now.duration_since(*ts) < self.ttl
        {
            return false;
        }
        seen.insert(nonce, now);
        true
    }
}

impl std::fmt::Debug for ReplayGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayGuard")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_body_accepted() {
        let guard = ReplayGuard::new();
        assert!(guard.check_and_record(b"first"));
    }

    #[test]
    fn duplicate_body_rejected() {
        let guard = ReplayGuard::new();
        assert!(guard.check_and_record(b"same"));
        assert!(!guard.check_and_record(b"same"));
    }

    #[test]
    fn different_bodies_both_accepted() {
        let guard = ReplayGuard::new();
        assert!(guard.check_and_record(b"one"));
        assert!(guard.check_and_record(b"two"));
    }
}
