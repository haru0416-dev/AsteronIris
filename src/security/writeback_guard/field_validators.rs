use super::constants::MAX_LIST_ITEM_CHARS;
use super::profile_validators::contains_poison_pattern;
use super::types::WritebackGuardVerdict;
use crate::llm::sanitize_api_error;
use serde_json::{Map, Value};

pub(super) type ValidationResult<T> = std::result::Result<T, String>;

pub(super) fn reject(reason: &str) -> WritebackGuardVerdict {
    WritebackGuardVerdict::Rejected {
        reason: sanitize_api_error(reason),
    }
}

pub(super) fn ensure_no_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    context: &str,
) -> ValidationResult<()> {
    for key in object.keys() {
        if !allowed.iter().any(|allowed_key| allowed_key == key) {
            return Err(format!("{context} contains unknown field: {key}"));
        }
    }
    Ok(())
}

pub(super) fn validate_string_field(
    object: &Map<String, Value>,
    field: &str,
    max_chars: usize,
    context: &str,
) -> ValidationResult<String> {
    let value = object
        .get(field)
        .ok_or_else(|| format!("{context}.{field} is required"))?;

    let raw = value
        .as_str()
        .ok_or_else(|| format!("{context}.{field} must be a string"))?;

    let sanitized = raw.trim().to_string();
    if sanitized.is_empty() {
        return Err(format!("{context}.{field} cannot be empty"));
    }
    if sanitized.chars().count() > max_chars {
        return Err(format!(
            "{context}.{field} exceeds max length ({max_chars})"
        ));
    }
    if contains_poison_pattern(&sanitized) {
        return Err(format!("{context}.{field} contains unsafe content pattern"));
    }

    Ok(sanitized)
}

pub(super) fn validate_string_array_field(
    object: &Map<String, Value>,
    field: &str,
    max_items: usize,
    context: &str,
) -> ValidationResult<Vec<String>> {
    let value = object
        .get(field)
        .ok_or_else(|| format!("{context}.{field} is required"))?;

    let list = value
        .as_array()
        .ok_or_else(|| format!("{context}.{field} must be an array"))?;

    if list.len() > max_items {
        return Err(format!("{context}.{field} exceeds max items ({max_items})"));
    }

    let mut out = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let raw = item
            .as_str()
            .ok_or_else(|| format!("{context}.{field}[{index}] must be a string"))?;

        let sanitized = raw.trim().to_string();
        if sanitized.is_empty() {
            return Err(format!("{context}.{field}[{index}] cannot be empty"));
        }
        if sanitized.chars().count() > MAX_LIST_ITEM_CHARS {
            return Err(format!(
                "{context}.{field}[{index}] exceeds max length ({MAX_LIST_ITEM_CHARS})"
            ));
        }
        if contains_poison_pattern(&sanitized) {
            return Err(format!(
                "{context}.{field}[{index}] contains unsafe content pattern"
            ));
        }

        out.push(sanitized);
    }

    Ok(out)
}
