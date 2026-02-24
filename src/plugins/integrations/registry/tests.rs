use super::*;
use crate::config::Config;
use crate::config::{IMessageConfig, MatrixConfig, TelegramConfig};
use crate::plugins::integrations::{IntegrationCategory, IntegrationStatus};

#[test]
fn registry_has_entries() {
    let entries = all_integrations();
    assert!(
        entries.len() >= 50,
        "Expected 50+ integrations, got {}",
        entries.len()
    );
}

#[test]
fn all_categories_represented() {
    let entries = all_integrations();
    for cat in IntegrationCategory::all() {
        let count = entries.iter().filter(|e| e.category == *cat).count();
        assert!(count > 0, "Category {cat:?} has no entries");
    }
}

#[test]
fn status_functions_dont_panic() {
    let config = Config::default();
    let entries = all_integrations();
    for entry in &entries {
        let _ = (entry.status_fn)(&config);
    }
}

#[test]
fn no_duplicate_names() {
    let entries = all_integrations();
    let mut seen = std::collections::HashSet::new();
    for entry in &entries {
        assert!(
            seen.insert(entry.name),
            "Duplicate integration name: {}",
            entry.name
        );
    }
}

#[test]
fn no_empty_names_or_descriptions() {
    let entries = all_integrations();
    for entry in &entries {
        assert!(!entry.name.is_empty(), "Found integration with empty name");
        assert!(
            !entry.description.is_empty(),
            "Integration '{}' has empty description",
            entry.name
        );
    }
}

#[test]
fn telegram_active_when_configured() {
    let mut config = Config::default();
    config.channels_config.telegram = Some(TelegramConfig {
        bot_token: "123:ABC".into(),
        allowed_users: vec!["user".into()],
        autonomy_level: None,
        tool_allowlist: None,
    });
    let entries = all_integrations();
    let tg = entries.iter().find(|e| e.name == "Telegram").unwrap();
    assert!(matches!((tg.status_fn)(&config), IntegrationStatus::Active));
}

#[test]
fn telegram_available_when_not_configured() {
    let config = Config::default();
    let entries = all_integrations();
    let tg = entries.iter().find(|e| e.name == "Telegram").unwrap();
    assert!(matches!(
        (tg.status_fn)(&config),
        IntegrationStatus::Available
    ));
}

#[test]
fn imessage_active_when_configured() {
    let mut config = Config::default();
    config.channels_config.imessage = Some(IMessageConfig {
        allowed_contacts: vec!["*".into()],
        autonomy_level: None,
        tool_allowlist: None,
    });
    let entries = all_integrations();
    let im = entries.iter().find(|e| e.name == "iMessage").unwrap();
    assert!(matches!((im.status_fn)(&config), IntegrationStatus::Active));
}

#[test]
fn imessage_available_when_not_configured() {
    let config = Config::default();
    let entries = all_integrations();
    let im = entries.iter().find(|e| e.name == "iMessage").unwrap();
    assert!(matches!(
        (im.status_fn)(&config),
        IntegrationStatus::Available
    ));
}

#[test]
fn matrix_active_when_configured() {
    let mut config = Config::default();
    config.channels_config.matrix = Some(MatrixConfig {
        homeserver: "https://m.org".into(),
        access_token: "tok".into(),
        room_id: "!r:m".into(),
        allowed_users: vec![],
        autonomy_level: None,
        tool_allowlist: None,
    });
    let entries = all_integrations();
    let mx = entries.iter().find(|e| e.name == "Matrix").unwrap();
    assert!(matches!((mx.status_fn)(&config), IntegrationStatus::Active));
}

#[test]
fn matrix_available_when_not_configured() {
    let config = Config::default();
    let entries = all_integrations();
    let mx = entries.iter().find(|e| e.name == "Matrix").unwrap();
    assert!(matches!(
        (mx.status_fn)(&config),
        IntegrationStatus::Available
    ));
}

#[test]
fn coming_soon_integrations_stay_coming_soon() {
    let config = Config::default();
    let entries = all_integrations();
    for name in ["WhatsApp", "Signal", "Nostr", "Spotify", "Home Assistant"] {
        let entry = entries.iter().find(|e| e.name == name).unwrap();
        assert!(
            matches!((entry.status_fn)(&config), IntegrationStatus::ComingSoon),
            "{name} should be ComingSoon"
        );
    }
}

#[test]
fn shell_and_filesystem_always_active() {
    let config = Config::default();
    let entries = all_integrations();
    for name in ["Shell", "File System"] {
        let entry = entries.iter().find(|e| e.name == name).unwrap();
        assert!(
            matches!((entry.status_fn)(&config), IntegrationStatus::Active),
            "{name} should always be Active"
        );
    }
}

#[test]
fn macos_active_on_macos() {
    let config = Config::default();
    let entries = all_integrations();
    let macos = entries.iter().find(|e| e.name == "macOS").unwrap();
    let status = (macos.status_fn)(&config);
    if cfg!(target_os = "macos") {
        assert!(matches!(status, IntegrationStatus::Active));
    } else {
        assert!(matches!(status, IntegrationStatus::Available));
    }
}

#[test]
fn category_counts_reasonable() {
    let entries = all_integrations();
    let chat_count = entries
        .iter()
        .filter(|e| e.category == IntegrationCategory::Chat)
        .count();
    let ai_count = entries
        .iter()
        .filter(|e| e.category == IntegrationCategory::AiModel)
        .count();
    assert!(
        chat_count >= 5,
        "Expected 5+ chat integrations, got {chat_count}"
    );
    assert!(
        ai_count >= 5,
        "Expected 5+ AI model integrations, got {ai_count}"
    );
}
