use crate::config::PersonaConfig;
use anyhow::{Result, bail};
use chrono::DateTime;
use serde::{Deserialize, Serialize};

pub const STATE_HEADER_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateHeaderV1 {
    pub schema_version: u8,
    pub identity_principles_hash: String,
    pub safety_posture: String,
    pub current_objective: String,
    pub open_loops: Vec<String>,
    pub next_actions: Vec<String>,
    pub commitments: Vec<String>,
    pub recent_context_summary: String,
    pub last_updated_at: String,
}

impl StateHeaderV1 {
    pub fn validate(&self, persona: &PersonaConfig) -> Result<()> {
        if self.schema_version != STATE_HEADER_SCHEMA_VERSION {
            bail!(
                "invalid schema_version: expected {}, got {}",
                STATE_HEADER_SCHEMA_VERSION,
                self.schema_version
            );
        }

        validate_non_empty("identity_principles_hash", &self.identity_principles_hash)?;
        validate_non_empty("safety_posture", &self.safety_posture)?;
        validate_text_len(
            "current_objective",
            &self.current_objective,
            persona.max_current_objective_chars,
        )?;
        validate_text_len(
            "recent_context_summary",
            &self.recent_context_summary,
            persona.max_recent_context_summary_chars,
        )?;

        validate_items(
            "open_loops",
            &self.open_loops,
            persona.max_open_loops,
            persona.max_list_item_chars,
        )?;
        validate_items(
            "next_actions",
            &self.next_actions,
            persona.max_next_actions,
            persona.max_list_item_chars,
        )?;
        validate_items(
            "commitments",
            &self.commitments,
            persona.max_commitments,
            persona.max_list_item_chars,
        )?;

        if DateTime::parse_from_rfc3339(&self.last_updated_at).is_err() {
            bail!("last_updated_at must be RFC3339");
        }

        Ok(())
    }

    pub fn validate_writeback_candidate(
        previous: &Self,
        candidate: &Self,
        persona: &PersonaConfig,
    ) -> Result<()> {
        candidate.validate(persona)?;

        if candidate.schema_version != previous.schema_version {
            bail!("immutable field changed: schema_version");
        }
        if candidate.identity_principles_hash != previous.identity_principles_hash {
            bail!("immutable field changed: identity_principles_hash");
        }
        if candidate.safety_posture != previous.safety_posture {
            bail!("immutable field changed: safety_posture");
        }

        Ok(())
    }
}

fn validate_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn validate_text_len(field: &str, value: &str, max_len: usize) -> Result<()> {
    validate_non_empty(field, value)?;
    if value.chars().count() > max_len {
        bail!("{field} exceeds max length of {max_len}");
    }
    Ok(())
}

fn validate_items(
    field: &str,
    items: &[String],
    max_items: usize,
    max_item_len: usize,
) -> Result<()> {
    if items.len() > max_items {
        bail!("{field} exceeds max items of {max_items}");
    }

    for item in items {
        if item.trim().is_empty() {
            bail!("{field} contains empty item");
        }
        if item.chars().count() > max_item_len {
            bail!("{field} item exceeds max length of {max_item_len}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state_header() -> StateHeaderV1 {
        StateHeaderV1 {
            schema_version: 1,
            identity_principles_hash: "abc123".into(),
            safety_posture: "strict".into(),
            current_objective: "Ship contracts for persona loop".into(),
            open_loops: vec!["Add strict schema".into()],
            next_actions: vec!["Implement validation".into()],
            commitments: vec!["Keep main-session scope".into()],
            recent_context_summary: "Task 1 focuses on config/schema contracts only.".into(),
            last_updated_at: "2026-02-16T10:00:00Z".into(),
        }
    }

    #[test]
    fn state_header_valid_v1_payload_parse() {
        let payload = r#"
{
  "schema_version": 1,
  "identity_principles_hash": "abc123",
  "safety_posture": "strict",
  "current_objective": "Ship contracts for persona loop",
  "open_loops": ["Add strict schema"],
  "next_actions": ["Implement validation"],
  "commitments": ["Keep main-session scope"],
  "recent_context_summary": "Task 1 focuses on config/schema contracts only.",
  "last_updated_at": "2026-02-16T10:00:00Z"
}
"#;

        let parsed: StateHeaderV1 = serde_json::from_str(payload).unwrap();
        parsed.validate(&PersonaConfig::default()).unwrap();
    }

    #[test]
    fn state_header_rejects_invalid() {
        let missing_required = r#"
{
  "schema_version": 1,
  "identity_principles_hash": "abc123",
  "safety_posture": "strict",
  "open_loops": [],
  "next_actions": [],
  "commitments": [],
  "recent_context_summary": "ok",
  "last_updated_at": "2026-02-16T10:00:00Z"
}
"#;

        let err = serde_json::from_str::<StateHeaderV1>(missing_required).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing field `current_objective`"),
            "unexpected serde error: {err}"
        );

        let mut invalid = sample_state_header();
        invalid.schema_version = 2;
        let validate_err = invalid.validate(&PersonaConfig::default()).unwrap_err();
        assert_eq!(
            validate_err.to_string(),
            "invalid schema_version: expected 1, got 2"
        );
    }

    #[test]
    fn state_header_rejects_immutable_field_mutation_boundary() {
        let previous = sample_state_header();
        let mut candidate = previous.clone();
        candidate.identity_principles_hash = "changed".into();

        let err = StateHeaderV1::validate_writeback_candidate(
            &previous,
            &candidate,
            &PersonaConfig::default(),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "immutable field changed: identity_principles_hash"
        );
    }
}
