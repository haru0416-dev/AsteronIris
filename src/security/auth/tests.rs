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
fn broker_prefers_provider_profile_over_config_api_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut config = test_config(&tmp);
    config.default_provider = Some("openrouter".into());
    config.api_key = Some("sk-config".into());
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
        Some("sk-config")
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
    config.api_key = Some("sk-config".into());
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

#[test]
fn active_profile_prefers_configured_order_when_default_missing() {
    let mut store = AuthProfileStore::default();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-a".into(),
                provider: "openai".into(),
                label: None,
                api_key: Some("sk-a".into()),
                refresh_token: None,
                auth_scheme: Some("api_key".into()),
                oauth_source: None,
                disabled: false,
            },
            false,
        )
        .unwrap();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-b".into(),
                provider: "openai".into(),
                label: None,
                api_key: Some("sk-b".into()),
                refresh_token: None,
                auth_scheme: Some("api_key".into()),
                oauth_source: None,
                disabled: false,
            },
            false,
        )
        .unwrap();

    store.defaults.insert("openai".into(), "missing-id".into());
    let order = vec!["openai-b".to_string(), "openai-a".to_string()];
    store.set_profile_order("openai", &order);

    let active = store.active_profile_for_provider("openai").unwrap();
    assert_eq!(active.id, "openai-b");
}

#[test]
fn active_profile_skips_cooldown_and_falls_back_to_ready_profile() {
    let mut store = AuthProfileStore::default();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-a".into(),
                provider: "openai".into(),
                label: None,
                api_key: Some("sk-a".into()),
                refresh_token: None,
                auth_scheme: Some("api_key".into()),
                oauth_source: None,
                disabled: false,
            },
            false,
        )
        .unwrap();
    store
        .upsert_profile(
            AuthProfile {
                id: "openai-b".into(),
                provider: "openai".into(),
                label: None,
                api_key: Some("sk-b".into()),
                refresh_token: None,
                auth_scheme: Some("api_key".into()),
                oauth_source: None,
                disabled: false,
            },
            false,
        )
        .unwrap();

    let order = vec!["openai-a".to_string(), "openai-b".to_string()];
    store.set_profile_order("openai", &order);
    store.mark_profile_failed("openai-a", Some(600));

    let active = store.active_profile_for_provider("openai").unwrap();
    assert_eq!(active.id, "openai-b");
}

#[test]
fn mark_profile_used_updates_last_good_and_usage_stats() {
    let mut store = AuthProfileStore::default();
    store
        .upsert_profile(
            AuthProfile {
                id: "anthropic-main".into(),
                provider: "anthropic".into(),
                label: None,
                api_key: Some("sk-ant".into()),
                refresh_token: None,
                auth_scheme: Some("api_key".into()),
                oauth_source: None,
                disabled: false,
            },
            false,
        )
        .unwrap();

    store.mark_profile_failed("anthropic-main", Some(60));
    store.mark_profile_used("anthropic", "anthropic-main");

    assert_eq!(
        store.last_good.get("anthropic").map(String::as_str),
        Some("anthropic-main")
    );
    let stats = store.usage_stats.get("anthropic-main").unwrap();
    assert_eq!(stats.error_count, 0);
    assert!(stats.last_used_at.is_some());
    assert!(stats.cooldown_until.is_none());
}
