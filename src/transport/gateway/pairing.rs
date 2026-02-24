//! Gateway-local pairing guard.
//!
//! Manages one-time pairing codes and bearer-token authentication for the
//! HTTP gateway. Tokens are stored as SHA-256 hashes (never plaintext).
//!
//! TODO: Consolidate with top-level `security::pairing` once that module is
//! ported to v2.

use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::Instant;

/// Maximum failed pairing attempts before lockout.
const MAX_FAILURES: u32 = 5;
/// Lockout duration in seconds after too many failures.
const LOCKOUT_SECS: u64 = 300;

/// SHA-256 hash a token for storage (never store plaintext).
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Guard that manages gateway pairing (one-time code exchange) and bearer
/// token validation.
pub struct PairingGuard {
    require_pairing: bool,
    paired_hashes: Mutex<Vec<String>>,
    pairing_code: Mutex<Option<String>>,
    failure_count: Mutex<u32>,
    lockout_until: Mutex<Option<Instant>>,
    token_ttl_secs: Option<u64>,
}

impl PairingGuard {
    pub fn new(
        require_pairing: bool,
        initial_hashes: &[String],
        token_ttl_secs: Option<u64>,
    ) -> Self {
        let code = if require_pairing && initial_hashes.is_empty() {
            Some(generate_pairing_code())
        } else {
            None
        };

        Self {
            require_pairing,
            paired_hashes: Mutex::new(initial_hashes.to_vec()),
            pairing_code: Mutex::new(code),
            failure_count: Mutex::new(0),
            lockout_until: Mutex::new(None),
            token_ttl_secs,
        }
    }

    pub fn require_pairing(&self) -> bool {
        self.require_pairing
    }

    /// Returns the one-time pairing code if it has not yet been consumed.
    pub fn pairing_code(&self) -> Option<String> {
        self.pairing_code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Returns `true` if at least one token has been paired.
    pub fn is_paired(&self) -> bool {
        let hashes = self
            .paired_hashes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !hashes.is_empty()
    }

    /// Validate a bearer token against stored hashes.
    pub fn is_authenticated(&self, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        let hash = hash_token(token);
        let hashes = self
            .paired_hashes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        hashes.iter().any(|h| h == &hash)
    }

    /// Attempt to pair with the given code.
    ///
    /// Returns:
    /// - `Ok(Some(token))` on success (code matched, token issued)
    /// - `Ok(None)` if the code was wrong
    /// - `Err(remaining_lockout_secs)` if locked out
    pub fn try_pair(&self, code: &str) -> Result<Option<String>, u64> {
        // Check lockout
        {
            let lockout = self
                .lockout_until
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(until) = *lockout {
                let remaining = until.saturating_duration_since(Instant::now());
                if !remaining.is_zero() {
                    return Err(remaining.as_secs().max(1));
                }
            }
        }

        let expected = {
            let guard = self
                .pairing_code
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.clone()
        };

        let Some(expected_code) = expected else {
            return Ok(None);
        };

        if !constant_time_eq(code, &expected_code) {
            let mut failures = self
                .failure_count
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *failures += 1;
            if *failures >= MAX_FAILURES {
                let mut lockout = self
                    .lockout_until
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *lockout = Some(Instant::now() + std::time::Duration::from_secs(LOCKOUT_SECS));
                *failures = 0;
                return Err(LOCKOUT_SECS);
            }
            return Ok(None);
        }

        // Success: consume the code and issue a token
        {
            let mut guard = self
                .pairing_code
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = None;
        }

        let token = generate_bearer_token();
        let hash = hash_token(&token);
        {
            let mut hashes = self
                .paired_hashes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            hashes.push(hash);
        }

        // Reset failure counter
        {
            let mut failures = self
                .failure_count
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *failures = 0;
        }

        let _ = self.token_ttl_secs; // Reserved for future TTL enforcement

        Ok(Some(token))
    }
}

/// Constant-time equality comparison for secret strings.
fn constant_time_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn generate_pairing_code() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 4];
    rand::rng().fill_bytes(&mut buf);
    let raw = u32::from_le_bytes(buf) % 1_000_000;
    format!("{raw:06}")
}

fn generate_bearer_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pairing_guard_generates_code_when_no_hashes() {
        let guard = PairingGuard::new(true, &[], None);
        assert!(guard.pairing_code().is_some());
        assert!(!guard.is_paired());
    }

    #[test]
    fn pairing_guard_with_existing_hashes_is_paired() {
        let hash = hash_token("existing-token");
        let guard = PairingGuard::new(true, &[hash], None);
        assert!(guard.is_paired());
        assert!(guard.pairing_code().is_none());
    }

    #[test]
    fn try_pair_success() {
        let guard = PairingGuard::new(true, &[], None);
        let code = guard.pairing_code().unwrap();
        let result = guard.try_pair(&code);
        assert!(result.is_ok());
        let token = result.unwrap();
        assert!(token.is_some());
        let token = token.unwrap();
        assert!(guard.is_authenticated(&token));
        assert!(guard.is_paired());
        // Code is consumed
        assert!(guard.pairing_code().is_none());
    }

    #[test]
    fn try_pair_wrong_code() {
        let guard = PairingGuard::new(true, &[], None);
        let result = guard.try_pair("wrong-code");
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn is_authenticated_rejects_empty() {
        let guard = PairingGuard::new(false, &[], None);
        assert!(!guard.is_authenticated(""));
    }

    #[test]
    fn hash_token_is_deterministic() {
        let h1 = hash_token("test");
        let h2 = hash_token("test");
        assert_eq!(h1, h2);
    }
}
