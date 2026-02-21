//! Provenance tracking — commit SHA pinning and content hashing.

use serde::{Deserialize, Serialize};

/// Provenance record for a skill artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// Git commit SHA the skill was fetched from.
    pub commit_sha: Option<String>,
    /// SHA-256 hash of the skill content at fetch time.
    pub content_hash: Option<String>,
    /// ISO-8601 timestamp when the skill was fetched.
    pub fetch_timestamp: Option<String>,
    /// Source URL the skill was fetched from.
    pub source_url: Option<String>,
}

impl Provenance {
    /// Creates an empty provenance (no data yet).
    pub fn empty() -> Self {
        Self {
            commit_sha: None,
            content_hash: None,
            fetch_timestamp: None,
            source_url: None,
        }
    }

    /// Returns true if the provenance has a pinned commit SHA (not a mutable ref like "main").
    pub fn has_pinned_ref(&self) -> bool {
        self.commit_sha
            .as_ref()
            .is_some_and(|sha| is_commit_sha(sha))
    }

    /// Returns true if content hash is present.
    pub fn has_content_hash(&self) -> bool {
        self.content_hash
            .as_ref()
            .is_some_and(|h| h.starts_with("sha256:") && h.len() > 7)
    }

    /// Verify that a given content hash matches the stored one.
    pub fn verify_content_hash(&self, computed_hash: &str) -> bool {
        self.content_hash
            .as_ref()
            .is_some_and(|stored| stored == computed_hash)
    }
}

/// Check if a string looks like a full commit SHA (40 hex chars) or short SHA (7+ hex chars).
fn is_commit_sha(s: &str) -> bool {
    let len = s.len();
    (7..=40).contains(&len) && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Known mutable ref names that should NOT be accepted as pinned references.
const MUTABLE_REFS: &[&str] = &[
    "main", "master", "develop", "dev", "HEAD", "latest", "trunk",
];

/// Returns true if the reference is a mutable branch name (not a commit SHA).
pub fn is_mutable_ref(reference: &str) -> bool {
    MUTABLE_REFS
        .iter()
        .any(|r| r.eq_ignore_ascii_case(reference))
        || !is_commit_sha(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_provenance_has_no_pinned_ref() {
        let prov = Provenance::empty();
        assert!(!prov.has_pinned_ref());
        assert!(!prov.has_content_hash());
    }

    #[test]
    fn full_sha_is_pinned() {
        let prov = Provenance {
            commit_sha: Some("abcdef1234567890abcdef1234567890abcdef12".into()),
            ..Provenance::empty()
        };
        assert!(prov.has_pinned_ref());
    }

    #[test]
    fn short_sha_is_pinned() {
        let prov = Provenance {
            commit_sha: Some("abc1234".into()),
            ..Provenance::empty()
        };
        assert!(prov.has_pinned_ref());
    }

    #[test]
    fn branch_name_is_not_pinned() {
        for branch in [
            "main", "master", "develop", "dev", "HEAD", "latest", "trunk",
        ] {
            let prov = Provenance {
                commit_sha: Some(branch.into()),
                ..Provenance::empty()
            };
            assert!(
                !prov.has_pinned_ref(),
                "branch '{branch}' should not be pinned"
            );
        }
    }

    #[test]
    fn content_hash_validation() {
        let prov = Provenance {
            content_hash: Some("sha256:abcdef0123456789".into()),
            ..Provenance::empty()
        };
        assert!(prov.has_content_hash());
        assert!(prov.verify_content_hash("sha256:abcdef0123456789"));
        assert!(!prov.verify_content_hash("sha256:different"));
    }

    #[test]
    fn content_hash_without_prefix_invalid() {
        let prov = Provenance {
            content_hash: Some("abcdef0123456789".into()),
            ..Provenance::empty()
        };
        assert!(!prov.has_content_hash());
    }

    #[test]
    fn is_mutable_ref_detects_branches() {
        assert!(is_mutable_ref("main"));
        assert!(is_mutable_ref("MAIN"));
        assert!(is_mutable_ref("Master"));
        assert!(is_mutable_ref("develop"));
        assert!(is_mutable_ref("HEAD"));
    }

    #[test]
    fn is_mutable_ref_allows_shas() {
        assert!(!is_mutable_ref("abcdef1234567890abcdef1234567890abcdef12"));
        assert!(!is_mutable_ref("abc1234"));
    }

    #[test]
    fn is_mutable_ref_rejects_short_strings() {
        assert!(is_mutable_ref("abc"));
        assert!(is_mutable_ref("v1.0"));
    }

    #[test]
    fn serde_roundtrip() {
        let prov = Provenance {
            commit_sha: Some("abc1234".into()),
            content_hash: Some("sha256:deadbeef".into()),
            fetch_timestamp: Some("2026-01-01T00:00:00Z".into()),
            source_url: Some("https://github.com/test/test".into()),
        };
        let json = serde_json::to_string(&prov).unwrap();
        let back: Provenance = serde_json::from_str(&json).unwrap();
        assert_eq!(back.commit_sha, prov.commit_sha);
        assert_eq!(back.content_hash, prov.content_hash);
    }
}
