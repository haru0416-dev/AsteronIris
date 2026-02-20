use super::constants::{
    ALLOWED_STATE_HEADER_FIELDS, ALLOWED_TOP_LEVEL_FIELDS, MAX_COMMITMENTS,
    MAX_CURRENT_OBJECTIVE_CHARS, MAX_LIST_ITEM_CHARS, MAX_MEMORY_APPEND_ITEM_CHARS,
    MAX_MEMORY_APPEND_ITEMS, MAX_NEXT_ACTIONS, MAX_OPEN_LOOPS, MAX_RECENT_CONTEXT_SUMMARY_CHARS,
    MAX_SELF_TASK_EXPIRY_HOURS, MAX_SELF_TASK_INSTRUCTIONS_CHARS, MAX_SELF_TASK_TITLE_CHARS,
    MAX_SELF_TASKS, POISON_PATTERNS, STYLE_SCORE_MAX, STYLE_SCORE_MIN, STYLE_TEMPERATURE_MAX,
    STYLE_TEMPERATURE_MIN,
};
use super::types::{
    ImmutableStateHeader, SelfTaskWriteback, StateHeaderWriteback, StyleProfileWriteback,
    WritebackGuardVerdict, WritebackPayload,
};
use crate::providers::sanitize_api_error;
use chrono::{DateTime, Duration, FixedOffset};
use serde_json::{Map, Value};

fn reject(reason: &str) -> WritebackGuardVerdict {
    WritebackGuardVerdict::Rejected {
        reason: sanitize_api_error(reason),
    }
}

type ValidationResult<T> = std::result::Result<T, String>;

fn ensure_no_unknown_fields(
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

fn validate_string_field(
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

fn validate_string_array_field(
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

fn validate_optional_memory_append(object: &Map<String, Value>) -> ValidationResult<Vec<String>> {
    let Some(memory_append) = object.get("memory_append") else {
        return Ok(Vec::new());
    };

    let entries = memory_append
        .as_array()
        .ok_or_else(|| "payload.memory_append must be an array".to_string())?;

    if entries.len() > MAX_MEMORY_APPEND_ITEMS {
        return Err(format!(
            "payload.memory_append exceeds max items ({MAX_MEMORY_APPEND_ITEMS})"
        ));
    }

    let mut out = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let raw = entry
            .as_str()
            .ok_or_else(|| format!("payload.memory_append[{index}] must be a string"))?;
        let sanitized = raw.trim().to_string();

        if sanitized.is_empty() {
            return Err(format!("payload.memory_append[{index}] cannot be empty"));
        }
        if sanitized.chars().count() > MAX_MEMORY_APPEND_ITEM_CHARS {
            return Err(format!(
                "payload.memory_append[{index}] exceeds max length ({MAX_MEMORY_APPEND_ITEM_CHARS})"
            ));
        }
        if contains_poison_pattern(&sanitized) {
            return Err(format!(
                "payload.memory_append[{index}] contains unsafe content pattern"
            ));
        }

        out.push(sanitized);
    }

    Ok(out)
}

fn validate_optional_self_tasks(
    object: &Map<String, Value>,
    state_last_updated_at: &str,
) -> ValidationResult<Vec<SelfTaskWriteback>> {
    let Some(raw_self_tasks) = object.get("self_tasks") else {
        return Ok(Vec::new());
    };

    let tasks = raw_self_tasks
        .as_array()
        .ok_or_else(|| "payload.self_tasks must be an array".to_string())?;
    if tasks.len() > MAX_SELF_TASKS {
        return Err(format!(
            "payload.self_tasks exceeds max items ({MAX_SELF_TASKS})"
        ));
    }

    let baseline = DateTime::<FixedOffset>::parse_from_rfc3339(state_last_updated_at)
        .map_err(|_| "payload.state_header.last_updated_at must be RFC3339".to_string())?;
    let max_expires_at = baseline + Duration::hours(MAX_SELF_TASK_EXPIRY_HOURS);

    let mut out = Vec::with_capacity(tasks.len());
    for (index, task) in tasks.iter().enumerate() {
        let task_obj = task
            .as_object()
            .ok_or_else(|| format!("payload.self_tasks[{index}] must be an object"))?;
        ensure_no_unknown_fields(
            task_obj,
            &["title", "instructions", "expires_at"],
            &format!("payload.self_tasks[{index}]"),
        )?;

        let title = validate_string_field(
            task_obj,
            "title",
            MAX_SELF_TASK_TITLE_CHARS,
            &format!("payload.self_tasks[{index}]"),
        )?;
        let instructions = validate_string_field(
            task_obj,
            "instructions",
            MAX_SELF_TASK_INSTRUCTIONS_CHARS,
            &format!("payload.self_tasks[{index}]"),
        )?;
        let expires_at = validate_string_field(
            task_obj,
            "expires_at",
            64,
            &format!("payload.self_tasks[{index}]"),
        )?;

        let parsed_expires_at = DateTime::<FixedOffset>::parse_from_rfc3339(&expires_at)
            .map_err(|_| format!("payload.self_tasks[{index}].expires_at must be RFC3339"))?;
        if parsed_expires_at <= baseline {
            return Err(format!(
                "payload.self_tasks[{index}].expires_at must be after payload.state_header.last_updated_at"
            ));
        }
        if parsed_expires_at > max_expires_at {
            return Err(format!(
                "payload.self_tasks[{index}].expires_at exceeds max horizon ({MAX_SELF_TASK_EXPIRY_HOURS}h)"
            ));
        }

        out.push(SelfTaskWriteback {
            title,
            instructions,
            expires_at,
        });
    }

    Ok(out)
}

fn validate_optional_style_profile(
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

fn contains_poison_pattern(input: &str) -> bool {
    let normalized = input.to_ascii_lowercase();
    POISON_PATTERNS
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

fn validate_last_updated_at(value: &str) -> ValidationResult<()> {
    DateTime::<FixedOffset>::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| "payload.state_header.last_updated_at must be RFC3339".to_string())
}

fn validate_state_header(
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

pub fn validate_writeback_payload(
    payload: &Value,
    immutable: &ImmutableStateHeader,
) -> WritebackGuardVerdict {
    let Some(root) = payload.as_object() else {
        return reject("payload must be a JSON object");
    };
    if let Err(reason) = ensure_no_unknown_fields(root, &ALLOWED_TOP_LEVEL_FIELDS, "payload") {
        return reject(&reason);
    }

    let Some(state_header_value) = root.get("state_header") else {
        return reject("payload.state_header is required");
    };
    let Some(state_header) = state_header_value.as_object() else {
        return reject("payload.state_header must be an object");
    };
    let state_header = match validate_state_header(state_header, immutable) {
        Ok(state_header) => state_header,
        Err(reason) => return reject(&reason),
    };

    let memory_append = match validate_optional_memory_append(root) {
        Ok(memory_append) => memory_append,
        Err(reason) => return reject(&reason),
    };
    let self_tasks = match validate_optional_self_tasks(root, &state_header.last_updated_at) {
        Ok(self_tasks) => self_tasks,
        Err(reason) => return reject(&reason),
    };
    let style_profile = match validate_optional_style_profile(root) {
        Ok(style_profile) => style_profile,
        Err(reason) => return reject(&reason),
    };

    WritebackGuardVerdict::Accepted(WritebackPayload {
        state_header,
        memory_append,
        self_tasks,
        style_profile,
    })
}
