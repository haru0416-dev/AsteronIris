use crate::config::Config;

const DEFAULT_PERSON_ID: &str = "local-default";

pub fn person_entity_id(person_id: &str) -> String {
    let normalized = sanitize_person_id(person_id);
    let effective = if normalized.is_empty() {
        DEFAULT_PERSON_ID.to_string()
    } else {
        normalized
    };
    format!("person:{effective}")
}

pub fn channel_person_entity_id(channel: &str, sender: &str) -> String {
    person_entity_id(&format!("{channel}.{sender}"))
}

pub fn resolve_person_id(config: &Config) -> String {
    if let Ok(from_env) = std::env::var("ASTERONIRIS_PERSON_ID") {
        let sanitized = sanitize_person_id(&from_env);
        if !sanitized.is_empty() {
            return sanitized;
        }
    }
    if let Some(from_config) = config.identity.person_id.as_deref() {
        let sanitized = sanitize_person_id(from_config);
        if !sanitized.is_empty() {
            return sanitized;
        }
    }
    DEFAULT_PERSON_ID.to_string()
}

pub fn sanitize_person_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_normalizes_unsafe_chars() {
        let result = sanitize_person_id("user@domain/path<>!");
        assert!(!result.contains('@'));
        assert!(!result.contains('/'));
        assert!(!result.contains('<'));
        assert!(!result.contains('>'));
        assert!(!result.contains('!'));
        assert_eq!(result, "user_domain_path");
    }

    #[test]
    fn sanitize_keeps_safe_tokens() {
        let result = sanitize_person_id("alice-bob_42.local");
        assert_eq!(result, "alice-bob_42.local");
    }

    #[test]
    fn person_entity_id_uses_default_for_empty() {
        assert_eq!(person_entity_id(""), "person:local-default");
        assert_eq!(person_entity_id("   "), "person:local-default");
    }

    #[test]
    fn channel_person_entity_id_combines_channel_and_sender() {
        let result = channel_person_entity_id("discord", "alice");
        assert_eq!(result, "person:discord.alice");
    }
}
