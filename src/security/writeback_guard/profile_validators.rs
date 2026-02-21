use super::constants::{
    ALLOWED_STATE_HEADER_FIELDS, MAX_COMMITMENTS, MAX_CURRENT_OBJECTIVE_CHARS, MAX_NEXT_ACTIONS,
    MAX_OPEN_LOOPS, MAX_RECENT_CONTEXT_SUMMARY_CHARS, POISON_PATTERNS, STYLE_SCORE_MAX,
    STYLE_SCORE_MIN, STYLE_TEMPERATURE_MAX, STYLE_TEMPERATURE_MIN,
};
use super::field_validators::{
    ValidationResult, ensure_no_unknown_fields, validate_string_array_field, validate_string_field,
};
use super::types::{ImmutableStateHeader, StateHeaderWriteback, StyleProfileWriteback};
use chrono::{DateTime, FixedOffset};
use serde_json::{Map, Value};

pub(super) fn validate_optional_style_profile(
    object: &Map<String, Value>,
) -> ValidationResult<Option<StyleProfileWriteback>> {
    let Some(raw_style_profile) = object.get("style_profile") else {
        return Ok(None);
    };

    let style_profile = raw_style_profile
        .as_object()
        .ok_or_else(|| "payload.style_profile must be an object".to_string())?;
    ensure_no_unknown_fields(
        style_profile,
        &["formality", "verbosity", "temperature"],
        "payload.style_profile",
    )?;

    let formality = style_profile
        .get("formality")
        .and_then(Value::as_u64)
        .ok_or_else(|| "payload.style_profile.formality must be an integer".to_string())?;
    if !(u64::from(STYLE_SCORE_MIN)..=u64::from(STYLE_SCORE_MAX)).contains(&formality) {
        return Err(format!(
            "payload.style_profile.formality must be in safe range [{STYLE_SCORE_MIN}, {STYLE_SCORE_MAX}]"
        ));
    }

    let verbosity = style_profile
        .get("verbosity")
        .and_then(Value::as_u64)
        .ok_or_else(|| "payload.style_profile.verbosity must be an integer".to_string())?;
    if !(u64::from(STYLE_SCORE_MIN)..=u64::from(STYLE_SCORE_MAX)).contains(&verbosity) {
        return Err(format!(
            "payload.style_profile.verbosity must be in safe range [{STYLE_SCORE_MIN}, {STYLE_SCORE_MAX}]"
        ));
    }

    let temperature = style_profile
        .get("temperature")
        .and_then(Value::as_f64)
        .ok_or_else(|| "payload.style_profile.temperature must be a number".to_string())?;
    if !(STYLE_TEMPERATURE_MIN..=STYLE_TEMPERATURE_MAX).contains(&temperature) {
        return Err(format!(
            "payload.style_profile.temperature must be in safe range [{STYLE_TEMPERATURE_MIN}, {STYLE_TEMPERATURE_MAX}]"
        ));
    }

    Ok(Some(StyleProfileWriteback {
        formality: u8::try_from(formality)
            .map_err(|_| "payload.style_profile.formality is out of range".to_string())?,
        verbosity: u8::try_from(verbosity)
            .map_err(|_| "payload.style_profile.verbosity is out of range".to_string())?,
        temperature,
    }))
}

pub(super) fn contains_poison_pattern(input: &str) -> bool {
    let normalized = input.to_ascii_lowercase();
    POISON_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

pub(super) fn validate_last_updated_at(value: &str) -> ValidationResult<()> {
    DateTime::<FixedOffset>::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| "payload.state_header.last_updated_at must be RFC3339".to_string())
}

pub(super) fn validate_state_header(
    state_header: &Map<String, Value>,
    immutable: &ImmutableStateHeader,
) -> ValidationResult<StateHeaderWriteback> {
    ensure_no_unknown_fields(
        state_header,
        &ALLOWED_STATE_HEADER_FIELDS,
        "payload.state_header",
    )?;

    let Some(schema_version_value) = state_header.get("schema_version").and_then(Value::as_u64)
    else {
        return Err("payload.state_header.schema_version must be an integer".to_string());
    };
    if schema_version_value != u64::from(immutable.schema_version) {
        return Err("immutable field mismatch: payload.state_header.schema_version".to_string());
    }

    let Some(identity_hash) = state_header
        .get("identity_principles_hash")
        .and_then(Value::as_str)
    else {
        return Err("payload.state_header.identity_principles_hash must be a string".to_string());
    };
    if identity_hash != immutable.identity_principles_hash {
        return Err(
            "immutable field mismatch: payload.state_header.identity_principles_hash".to_string(),
        );
    }

    let Some(safety_posture) = state_header.get("safety_posture").and_then(Value::as_str) else {
        return Err("payload.state_header.safety_posture must be a string".to_string());
    };
    if safety_posture != immutable.safety_posture {
        return Err("immutable field mismatch: payload.state_header.safety_posture".to_string());
    }

    let current_objective = validate_string_field(
        state_header,
        "current_objective",
        MAX_CURRENT_OBJECTIVE_CHARS,
        "payload.state_header",
    )?;
    let open_loops = validate_string_array_field(
        state_header,
        "open_loops",
        MAX_OPEN_LOOPS,
        "payload.state_header",
    )?;
    let next_actions = validate_string_array_field(
        state_header,
        "next_actions",
        MAX_NEXT_ACTIONS,
        "payload.state_header",
    )?;
    let commitments = validate_string_array_field(
        state_header,
        "commitments",
        MAX_COMMITMENTS,
        "payload.state_header",
    )?;
    let recent_context_summary = validate_string_field(
        state_header,
        "recent_context_summary",
        MAX_RECENT_CONTEXT_SUMMARY_CHARS,
        "payload.state_header",
    )?;
    let last_updated_at =
        validate_string_field(state_header, "last_updated_at", 64, "payload.state_header")?;
    validate_last_updated_at(&last_updated_at)?;

    Ok(StateHeaderWriteback {
        current_objective,
        open_loops,
        next_actions,
        commitments,
        recent_context_summary,
        last_updated_at,
    })
}
