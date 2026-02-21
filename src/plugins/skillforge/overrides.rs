//! Override mechanism for skill gate decisions.
//!
//! Allows operators to approve specific quarantine/reject reasons
//! for a known skill at a pinned commit + content hash.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── Override entry ───────────────────────────────────────────────────────────

/// A single override entry from `skill-overrides.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillOverride {
    /// Skill identifier (name or URL slug).
    pub skill_id: String,
    /// Pinned commit SHA this override applies to.
    pub commit_sha: String,
    /// Content hash this override applies to (`sha256:...`).
    pub content_hash: String,
    /// Reason code IDs that are approved (e.g. `["quarantine:subprocess"]`).
    pub rule_ids: Vec<String>,
    /// Who approved this override.
    pub approved_by: String,
    /// ISO-8601 timestamp of approval.
    pub approved_at: String,
    /// Free-text justification.
    #[serde(default)]
    pub reason: String,
}

/// Top-level structure for `skill-overrides.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillOverrides {
    #[serde(default, rename = "override")]
    pub overrides: Vec<SkillOverride>,
}

// ── Loading ──────────────────────────────────────────────────────────────────

/// Load overrides from a TOML file at `path`.
/// Returns an empty set if the file does not exist.
pub fn load_overrides(path: &Path) -> Result<SkillOverrides> {
    if !path.exists() {
        return Ok(SkillOverrides::default());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read overrides file: {}", path.display()))?;

    let overrides: SkillOverrides = toml::from_str(&content)
        .with_context(|| format!("failed to parse overrides file: {}", path.display()))?;

    Ok(overrides)
}

// ── Lookup ───────────────────────────────────────────────────────────────────

impl SkillOverrides {
    /// Find override rule IDs for a skill at a specific commit + content hash.
    /// Both `commit_sha` and `content_hash` must match for the override to apply.
    pub fn rule_ids_for(
        &self,
        skill_id: &str,
        commit_sha: Option<&str>,
        content_hash: Option<&str>,
    ) -> Vec<String> {
        self.overrides
            .iter()
            .filter(|o| {
                o.skill_id == skill_id
                    && commit_sha.is_some_and(|sha| sha == o.commit_sha)
                    && content_hash.is_some_and(|hash| hash == o.content_hash)
            })
            .flat_map(|o| o.rule_ids.clone())
            .collect()
    }

    /// Returns true if there are no overrides loaded.
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn empty_overrides() {
        let overrides = SkillOverrides::default();
        assert!(overrides.is_empty());
        assert!(overrides.rule_ids_for("test", None, None).is_empty());
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let path = std::path::Path::new("/tmp/nonexistent-overrides-12345.toml");
        let overrides = load_overrides(path).unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn load_valid_toml() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        write!(
            tmp.as_file(),
            r#"
[[override]]
skill_id = "test-skill"
commit_sha = "abc1234def5678901234567890123456789012ab"
content_hash = "sha256:deadbeef"
rule_ids = ["quarantine:subprocess", "quarantine:env_read"]
approved_by = "haru"
approved_at = "2026-02-21T00:00:00Z"
reason = "legitimate subprocess use"
"#
        )
        .unwrap();

        let overrides = load_overrides(tmp.path()).unwrap();
        assert!(!overrides.is_empty());
        assert_eq!(overrides.overrides.len(), 1);
        assert_eq!(overrides.overrides[0].skill_id, "test-skill");
        assert_eq!(overrides.overrides[0].rule_ids.len(), 2);
    }

    #[test]
    fn rule_ids_for_matches_on_sha_and_hash() {
        let overrides = SkillOverrides {
            overrides: vec![SkillOverride {
                skill_id: "my-skill".into(),
                commit_sha: "abc123".into(),
                content_hash: "sha256:fff".into(),
                rule_ids: vec!["Subprocess".into()],
                approved_by: "haru".into(),
                approved_at: "2026-01-01T00:00:00Z".into(),
                reason: "ok".into(),
            }],
        };

        let ids = overrides.rule_ids_for("my-skill", Some("abc123"), Some("sha256:fff"));
        assert_eq!(ids, vec!["Subprocess"]);

        let ids = overrides.rule_ids_for("my-skill", Some("wrong"), Some("sha256:fff"));
        assert!(ids.is_empty());

        let ids = overrides.rule_ids_for("my-skill", Some("abc123"), Some("sha256:wrong"));
        assert!(ids.is_empty());

        let ids = overrides.rule_ids_for("other-skill", Some("abc123"), Some("sha256:fff"));
        assert!(ids.is_empty());

        let ids = overrides.rule_ids_for("my-skill", None, Some("sha256:fff"));
        assert!(ids.is_empty());
    }

    #[test]
    fn load_invalid_toml_returns_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp.as_file(), "this is not valid toml {{{{").unwrap();
        assert!(load_overrides(tmp.path()).is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let entry = SkillOverride {
            skill_id: "test".into(),
            commit_sha: "abc".into(),
            content_hash: "sha256:def".into(),
            rule_ids: vec!["Subprocess".into()],
            approved_by: "user".into(),
            approved_at: "2026-01-01T00:00:00Z".into(),
            reason: "testing".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: SkillOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(back.skill_id, "test");
        assert_eq!(back.rule_ids, vec!["Subprocess"]);
    }
}
