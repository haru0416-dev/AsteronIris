use super::*;
use crate::config::Config;
use std::fs;

#[allow(clippy::field_reassign_with_default)]
fn test_config(tmp: &tempfile::TempDir) -> Config {
    let mut config = Config::default();
    config.config_path = tmp.path().join("config.toml");
    config.workspace_dir = tmp.path().join("workspace");
    config
}

#[test]
fn broker_migrates_legacy_config_api_key_to_profiles_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut config = test_config(&tmp);
    config.default_provider = Some("openrouter".into());
    config.api_key = Some("sk-legacy-openrouter".into());
    config.secrets.encrypt = true;

    let broker = AuthBroker::load_or_init(&config).unwrap();
    assert_eq!(
        broker.resolve_provider_api_key("openrouter").as_deref(),
        Some("sk-legacy-openrouter")
    );

    let store_path = auth_profiles_path(&config);
    assert!(store_path.exists());

    let persisted = fs::read_to_string(store_path).unwrap();
    assert!(persisted.contains("enc2:"));
    assert!(!persisted.contains("sk-legacy-openrouter"));
}

#[test]
fn broker_prefers_provider_profile_over_legacy_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut config = test_config(&tmp);
    config.default_provider = Some("openrouter".into());
    config.api_key = Some("sk-legacy".into());
    config.secrets.encrypt = true;

    let path = auth_profiles_path(&config);
    fs::write(
        &path,
        r#"{
  "version": 1,
  "defaults": {
    "openrouter": "or-default",
    "openai": "oa-default"
  },
  "profiles": [
    {
      "id": "or-default",
      "provider": "openrouter",
      "api_key": "sk-openrouter-profile"
    },
    {
      "id": "oa-default",
      "provider": "openai",
      "api_key": "sk-openai-profile"
    }
  ]
}"#,
    )
    .unwrap();

    let broker = AuthBroker::load_or_init(&config).unwrap();
    assert_eq!(
        broker.resolve_provider_api_key("openrouter").as_deref(),
        Some("sk-openrouter-profile")
    );
    assert_eq!(
        broker.resolve_provider_api_key("openai").as_deref(),
        Some("sk-openai-profile")
    );
    assert_eq!(
        broker.resolve_provider_api_key("anthropic").as_deref(),
        Some("sk-legacy")
    );

    let persisted = fs::read_to_string(path).unwrap();
    assert!(persisted.contains("enc2:"));
    assert!(!persisted.contains("sk-openrouter-profile"));
    assert!(!persisted.contains("sk-openai-profile"));
}

#[test]
fn broker_resolves_embedding_key_from_openai_profile() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut config = test_config(&tmp);
    config.default_provider = Some("openrouter".into());
    config.api_key = Some("sk-legacy".into());
    config.memory.embedding_provider = "openai".into();

    let path = auth_profiles_path(&config);
    fs::write(
        &path,
        r#"{
  "version": 1,
  "defaults": {
    "openai": "oa-default"
  },
  "profiles": [
    {
      "id": "oa-default",
      "provider": "openai",
      "api_key": "sk-openai-profile"
    }
  ]
}"#,
    )
    .unwrap();

    let broker = AuthBroker::load_or_init(&config).unwrap();
    assert_eq!(
        broker.resolve_memory_api_key(&config.memory).as_deref(),
        Some("sk-openai-profile")
    );
}

#[test]
fn upsert_profile_sets_provider_default_and_normalizes_values() {
    let mut store = AuthProfileStore::default();

    let created = store
        .upsert_profile(
            AuthProfile {
                id: "openai-main".into(),
                provider: "OpenAI".into(),
                label: Some("  Primary Key  ".into()),
                api_key: Some("  sk-openai-main  ".into()),
                refresh_token: Some("  refresh-main  ".into()),
                auth_scheme: Some("  OAuth  ".into()),
                oauth_source: Some("  codex  ".into()),
                disabled: true,
            },
            true,
        )
        .unwrap();

    assert!(created);
    assert_eq!(store.profiles.len(), 1);
    assert_eq!(store.profiles[0].provider, "openai");
    assert_eq!(store.profiles[0].label.as_deref(), Some("Primary Key"));
    assert_eq!(store.profiles[0].api_key.as_deref(), Some("sk-openai-main"));
    assert_eq!(
        store.profiles[0].refresh_token.as_deref(),
        Some("refresh-main")
    );
    assert_eq!(store.profiles[0].auth_scheme.as_deref(), Some("oauth"));
    assert_eq!(store.profiles[0].oauth_source.as_deref(), Some("codex"));
    assert!(!store.profiles[0].disabled);
    assert_eq!(
        store.defaults.get("openai"),
        Some(&"openai-main".to_string())
    );
}

#[test]
fn upsert_profile_rejects_invalid_id() {
    let mut store = AuthProfileStore::default();
    let result = store.upsert_profile(
        AuthProfile {
            id: "bad id".into(),
            provider: "openrouter".into(),
            label: None,
            api_key: Some("sk-test".into()),
            refresh_token: None,
            auth_scheme: Some("api_key".into()),
            oauth_source: None,
            disabled: false,
        },
        true,
    );

    assert!(result.is_err());
}

#[test]
fn load_or_init_migrates_legacy_key_without_duplicate_profiles() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut config = test_config(&tmp);
    config.default_provider = Some("openrouter".into());
    config.api_key = Some("sk-legacy-openrouter".into());
    config.secrets.encrypt = true;

    let store_first = AuthProfileStore::load_or_init_for_config(&config).unwrap();
    assert_eq!(store_first.profiles.len(), 1);

    let store_second = AuthProfileStore::load_or_init_for_config(&config).unwrap();
    assert_eq!(store_second.profiles.len(), 1);

    let persisted = fs::read_to_string(auth_profiles_path(&config)).unwrap();
    assert!(persisted.contains("enc2:"));
}

#[test]
fn save_encrypts_refresh_token_in_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut config = test_config(&tmp);
    config.secrets.encrypt = true;

    let mut store = AuthProfileStore::default();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-oauth".into(),
                provider: "openai".into(),
                label: Some("OAuth import".into()),
                api_key: Some("access-token-plaintext".into()),
                refresh_token: Some("refresh-token-plaintext".into()),
                auth_scheme: Some("oauth".into()),
                oauth_source: Some("codex".into()),
                disabled: false,
            },
            true,
        )
        .unwrap();

    store.save_for_config(&config).unwrap();
    let persisted = fs::read_to_string(auth_profiles_path(&config)).unwrap();

    assert!(persisted.contains("enc2:"));
    assert!(!persisted.contains("access-token-plaintext"));
    assert!(!persisted.contains("refresh-token-plaintext"));
}

#[test]
fn recover_oauth_profile_returns_false_for_non_oauth_profile() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = test_config(&tmp);

    let mut store = AuthProfileStore::default();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-main".into(),
                provider: "openai".into(),
                label: None,
                api_key: Some("sk-main".into()),
                refresh_token: None,
                auth_scheme: Some("api_key".into()),
                oauth_source: None,
                disabled: false,
            },
            true,
        )
        .unwrap();
    store.save_for_config(&config).unwrap();

    let recovered = recover_oauth_profile_for_provider(&config, "openai").unwrap();
    assert!(!recovered);
}

#[test]
fn recover_oauth_profile_returns_false_for_unknown_oauth_source() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = test_config(&tmp);

    let mut store = AuthProfileStore::default();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-oauth".into(),
                provider: "openai".into(),
                label: None,
                api_key: Some("access-old".into()),
                refresh_token: Some("refresh-old".into()),
                auth_scheme: Some("oauth".into()),
                oauth_source: Some("custom-source".into()),
                disabled: false,
            },
            true,
        )
        .unwrap();
    store.save_for_config(&config).unwrap();

    let recovered = recover_oauth_profile_for_provider(&config, "openai").unwrap();
    assert!(!recovered);
}
