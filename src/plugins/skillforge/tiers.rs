//! Skill trust tiers — determines runtime permission scope.

use serde::{Deserialize, Serialize};

/// Trust tier assigned to a skill after gate evaluation.
/// Higher tiers grant broader runtime permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTier {
    /// Gate rejected — skill must not execute.
    Blocked,
    /// New or unreviewed — no net/fs/env/run permissions.
    Sandboxed,
    /// Declared capabilities only — strict allowlist.
    Restricted,
    /// Broader grants — reviewed skills with good track record.
    Trusted,
    /// Manual audit + content hash verified.
    Verified,
}

impl SkillTier {
    /// Returns the default tier for newly discovered skills.
    pub fn default_tier() -> Self {
        Self::Sandboxed
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Sandboxed => "sandboxed",
            Self::Restricted => "restricted",
            Self::Trusted => "trusted",
            Self::Verified => "verified",
        }
    }
}

impl std::fmt::Display for SkillTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_ordering() {
        assert!(SkillTier::Blocked < SkillTier::Sandboxed);
        assert!(SkillTier::Sandboxed < SkillTier::Restricted);
        assert!(SkillTier::Restricted < SkillTier::Trusted);
        assert!(SkillTier::Trusted < SkillTier::Verified);
    }

    #[test]
    fn default_tier_is_sandboxed() {
        assert_eq!(SkillTier::default_tier(), SkillTier::Sandboxed);
    }

    #[test]
    fn label_matches_display() {
        for tier in [
            SkillTier::Blocked,
            SkillTier::Sandboxed,
            SkillTier::Restricted,
            SkillTier::Trusted,
            SkillTier::Verified,
        ] {
            assert_eq!(tier.label(), tier.to_string());
        }
    }

    #[test]
    fn serde_roundtrip() {
        let tier = SkillTier::Restricted;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"restricted\"");
        let back: SkillTier = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tier);
    }

    #[test]
    fn all_variants_serialize_snake_case() {
        for (tier, expected) in [
            (SkillTier::Blocked, "\"blocked\""),
            (SkillTier::Sandboxed, "\"sandboxed\""),
            (SkillTier::Restricted, "\"restricted\""),
            (SkillTier::Trusted, "\"trusted\""),
            (SkillTier::Verified, "\"verified\""),
        ] {
            assert_eq!(serde_json::to_string(&tier).unwrap(), expected);
        }
    }
}
