use super::*;
use tempfile::TempDir;

/// ChaCha20Poly1305 key length (32 bytes / 256 bits).
const KEY_LEN: usize = 32;

// ── SecretStore basics ─────────────────────────────────────

#[test]
fn encrypt_decrypt_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let secret = "sk-my-secret-api-key-12345";

    let encrypted = store.encrypt(secret).unwrap();
    assert!(encrypted.starts_with("enc2:"), "Should have enc2: prefix");
    assert_ne!(encrypted, secret, "Should not be plaintext");

    let decrypted = store.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, secret, "Roundtrip must preserve original");
}

#[test]
fn encrypt_empty_returns_empty() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let result = store.encrypt("").unwrap();
    assert_eq!(result, "");
}

#[test]
fn decrypt_plaintext_passthrough() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let result = store.decrypt("sk-plaintext-key").unwrap();
    assert_eq!(result, "sk-plaintext-key");
}

#[test]
fn disabled_store_returns_plaintext() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), false);
    let result = store.encrypt("sk-secret").unwrap();
    assert_eq!(result, "sk-secret", "Disabled store should not encrypt");
}

#[test]
fn is_encrypted_detects_prefix() {
    assert!(SecretStore::is_encrypted("enc2:aabbcc"));
    assert!(!SecretStore::is_encrypted("sk-plaintext"));
    assert!(!SecretStore::is_encrypted(""));
}

#[test]
fn key_file_created_on_first_encrypt() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    assert!(!store.key_path.exists());

    store.encrypt("test").unwrap();
    assert!(store.key_path.exists(), "Key file should be created");

    let key_hex = fs::read_to_string(&store.key_path).unwrap();
    assert_eq!(
        key_hex.len(),
        KEY_LEN * 2,
        "Key should be {KEY_LEN} bytes hex-encoded"
    );
}

#[test]
fn encrypting_same_value_produces_different_ciphertext() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);

    let e1 = store.encrypt("secret").unwrap();
    let e2 = store.encrypt("secret").unwrap();
    assert_ne!(
        e1, e2,
        "AEAD with random nonce should produce different ciphertext each time"
    );

    // Both should still decrypt to the same value
    assert_eq!(store.decrypt(&e1).unwrap(), "secret");
    assert_eq!(store.decrypt(&e2).unwrap(), "secret");
}

#[test]
fn different_stores_same_dir_interop() {
    let tmp = TempDir::new().unwrap();
    let store1 = SecretStore::new(tmp.path(), true);
    let store2 = SecretStore::new(tmp.path(), true);

    let encrypted = store1.encrypt("cross-store-secret").unwrap();
    let decrypted = store2.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, "cross-store-secret");
}

#[test]
fn unicode_secret_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let secret = "sk-日本語テスト-émojis-🦀";

    let encrypted = store.encrypt(secret).unwrap();
    let decrypted = store.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, secret);
}

#[test]
fn long_secret_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let secret = "a".repeat(10_000);

    let encrypted = store.encrypt(&secret).unwrap();
    let decrypted = store.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, secret);
}

#[test]
fn corrupt_hex_returns_error() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let result = store.decrypt("enc2:not-valid-hex!!");
    assert!(result.is_err());
}

#[test]
fn tampered_ciphertext_detected() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    let encrypted = store.encrypt("sensitive-data").unwrap();

    // Flip a bit in the ciphertext (after the "enc2:" prefix)
    let hex_str = &encrypted[5..];
    let mut blob = hex_decode(hex_str).unwrap();
    // Modify a byte in the ciphertext portion (after the 12-byte nonce)
    if blob.len() > NONCE_LEN {
        blob[NONCE_LEN] ^= 0xff;
    }
    let tampered = format!("enc2:{}", hex_encode(&blob));

    let result = store.decrypt(&tampered);
    assert!(result.is_err(), "Tampered ciphertext must be rejected");
}

#[test]
fn wrong_key_detected() {
    let tmp1 = TempDir::new().unwrap();
    let tmp2 = TempDir::new().unwrap();
    let store1 = SecretStore::new(tmp1.path(), true);
    let store2 = SecretStore::new(tmp2.path(), true);

    let encrypted = store1.encrypt("secret-for-store1").unwrap();
    let result = store2.decrypt(&encrypted);
    assert!(result.is_err(), "Decrypting with a different key must fail");
}

#[test]
fn truncated_ciphertext_returns_error() {
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    // Only a few bytes — shorter than nonce
    let result = store.decrypt("enc2:aabbccdd");
    assert!(result.is_err(), "Too-short ciphertext must be rejected");
}

// ── Low-level helpers ───────────────────────────────────────

#[test]
fn hex_roundtrip() {
    let data = vec![0x00, 0x01, 0xfe, 0xff, 0xab, 0xcd];
    let encoded = hex_encode(&data);
    assert_eq!(encoded, "0001feffabcd");
    let decoded = hex_decode(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn hex_decode_odd_length_fails() {
    assert!(hex_decode("abc").is_err());
}

#[test]
fn hex_decode_invalid_chars_fails() {
    assert!(hex_decode("zzzz").is_err());
}

#[cfg(windows)]
#[test]
fn windows_icacls_grant_arg_rejects_empty_username() {
    assert_eq!(build_windows_icacls_grant_arg(""), None);
    assert_eq!(build_windows_icacls_grant_arg("   \t\n"), None);
}

#[cfg(windows)]
#[test]
fn windows_icacls_grant_arg_trims_username() {
    assert_eq!(
        build_windows_icacls_grant_arg("  alice  "),
        Some("alice:F".to_string())
    );
}

#[cfg(windows)]
#[test]
fn windows_icacls_grant_arg_preserves_valid_characters() {
    assert_eq!(
        build_windows_icacls_grant_arg("DOMAIN\\svc-user"),
        Some("DOMAIN\\svc-user:F".to_string())
    );
}

#[test]
fn generate_random_key_correct_length() {
    let key = generate_random_key();
    assert_eq!(key.len(), KEY_LEN);
}

#[test]
fn generate_random_key_not_all_zeros() {
    let key = generate_random_key();
    assert!(key.iter().any(|&b| b != 0), "Key should not be all zeros");
}

#[test]
fn two_random_keys_differ() {
    let k1 = generate_random_key();
    let k2 = generate_random_key();
    assert_ne!(k1, k2, "Two random keys should differ");
}

#[test]
fn generate_random_key_has_no_uuid_fixed_bits() {
    // UUID v4 has fixed bits at positions 6 (version = 0b0100xxxx) and
    // 8 (variant = 0b10xxxxxx). A direct CSPRNG key should not consistently
    // have these patterns across multiple samples.
    let mut version_match = 0;
    let mut variant_match = 0;
    let samples = 100;
    for _ in 0..samples {
        let key = generate_random_key();
        // In UUID v4, byte 6 always has top nibble = 0x4
        if key[6] & 0xf0 == 0x40 {
            version_match += 1;
        }
        // In UUID v4, byte 8 always has top 2 bits = 0b10
        if key[8] & 0xc0 == 0x80 {
            variant_match += 1;
        }
    }
    // With true randomness, each pattern should appear ~1/16 and ~1/4 of
    // the time. UUID would hit 100/100 on both. Allow generous margin.
    assert!(
        version_match < 30,
        "byte[6] matched UUID v4 version nibble {version_match}/100 times — \
         likely still using UUID-based key generation"
    );
    assert!(
        variant_match < 50,
        "byte[8] matched UUID v4 variant bits {variant_match}/100 times — \
         likely still using UUID-based key generation"
    );
}

#[cfg(unix)]
#[test]
fn key_file_has_restricted_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().unwrap();
    let store = SecretStore::new(tmp.path(), true);
    store.encrypt("trigger key creation").unwrap();

    let perms = fs::metadata(&store.key_path).unwrap().permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "Key file must be owner-only (0600)"
    );
}

#[test]
fn key_material_uses_zeroizing_wrapper() {
    let key: zeroize::Zeroizing<Vec<u8>> = generate_random_key();
    assert!(!key.is_empty());
}
