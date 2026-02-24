use anyhow::{Context, Result};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, OsRng, rand_core::RngCore},
};
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

const KEY_FILE: &str = ".secret_key";
const ENC_PREFIX: &str = "ENC:";
const NONCE_LEN: usize = 12;

pub struct SecretStore {
    root: PathBuf,
    encrypt: bool,
}

impl SecretStore {
    pub fn new(root: &Path, encrypt: bool) -> Self {
        Self {
            root: root.to_path_buf(),
            encrypt,
        }
    }

    /// Returns `true` if the value has already been encrypted.
    #[must_use]
    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with(ENC_PREFIX)
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        if !self.encrypt || plaintext.is_empty() || Self::is_encrypted(plaintext) {
            return Ok(plaintext.to_string());
        }

        let mut key_bytes = self.load_or_create_key()?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes).context("invalid key length")?;
        key_bytes.zeroize();

        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

        let mut combined = nonce_bytes.to_vec();
        combined.extend_from_slice(&ciphertext);
        Ok(format!("{ENC_PREFIX}{}", hex::encode(combined)))
    }

    pub fn decrypt(&self, value: &str) -> Result<String> {
        if !Self::is_encrypted(value) {
            return Ok(value.to_string());
        }

        let hex_str = &value[ENC_PREFIX.len()..];
        let combined = hex::decode(hex_str).context("invalid hex in encrypted value")?;

        if combined.len() < NONCE_LEN {
            anyhow::bail!("encrypted value too short");
        }

        let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);

        let mut key_bytes = self.load_or_create_key()?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key_bytes).context("invalid key length")?;
        key_bytes.zeroize();

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;

        String::from_utf8(plaintext).context("decrypted value is not valid UTF-8")
    }

    fn key_path(&self) -> PathBuf {
        self.root.join(KEY_FILE)
    }

    fn load_or_create_key(&self) -> Result<Vec<u8>> {
        let path = self.key_path();
        if path.exists() {
            let hex_key = fs::read_to_string(&path).context("failed to read key file")?;
            let key = hex::decode(hex_key.trim()).context("invalid hex in key file")?;
            if key.len() != 32 {
                anyhow::bail!("key file has invalid length (expected 32 bytes)");
            }
            Ok(key)
        } else {
            let mut key = vec![0u8; 32];
            OsRng.fill_bytes(&mut key);
            fs::write(&path, hex::encode(&key)).context("failed to write key file")?;
            Ok(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = SecretStore::new(dir.path(), true);

        let plaintext = "sk-test-secret-key-12345";
        let encrypted = store.encrypt(plaintext).unwrap();
        assert!(SecretStore::is_encrypted(&encrypted));
        assert_ne!(encrypted, plaintext);

        let decrypted = store.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn passthrough_when_encryption_disabled() {
        let dir = TempDir::new().unwrap();
        let store = SecretStore::new(dir.path(), false);

        let plaintext = "sk-not-encrypted";
        let result = store.encrypt(plaintext).unwrap();
        assert_eq!(result, plaintext);
    }

    #[test]
    fn decrypt_plaintext_returns_as_is() {
        let dir = TempDir::new().unwrap();
        let store = SecretStore::new(dir.path(), true);

        let plaintext = "not-encrypted-value";
        let result = store.decrypt(plaintext).unwrap();
        assert_eq!(result, plaintext);
    }

    #[test]
    fn is_encrypted_detects_prefix() {
        assert!(SecretStore::is_encrypted("ENC:abcdef1234"));
        assert!(!SecretStore::is_encrypted("plaintext"));
        assert!(!SecretStore::is_encrypted(""));
    }
}
