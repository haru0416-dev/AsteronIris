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
    fn sanitize_person_id_normalizes_unsafe_characters() {
        assert_eq!(sanitize_person_id(" user/a b:c "), "user_a_b_c");
    }

    #[test]
    fn sanitize_person_id_keeps_safe_ascii_tokens() {
        assert_eq!(sanitize_person_id("alice-01.dev"), "alice-01.dev");
    }
}
