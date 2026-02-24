use serde::{Deserialize, Serialize};

/// Describes how two memory entries are related.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationKind {
    /// The source entry is topically related to the target.
    RelatedTo,
    /// The source entry supersedes or updates the target.
    Updates,
    /// The source entry contradicts the target.
    Contradicts,
    /// The source entry was caused by the target.
    CausedBy,
}

/// A directed edge between two memory entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAssociation {
    pub source_id: String,
    pub target_id: String,
    pub kind: AssociationKind,
    pub confidence: f64,
    pub created_at: String,
}

impl MemoryAssociation {
    pub fn new(
        source_id: impl Into<String>,
        target_id: impl Into<String>,
        kind: AssociationKind,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            target_id: target_id.into(),
            kind,
            confidence: 1.0,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn association_serde_roundtrip() {
        let assoc = MemoryAssociation::new("a", "b", AssociationKind::Updates);
        let json = serde_json::to_value(&assoc).unwrap();
        let decoded: MemoryAssociation = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.source_id, "a");
        assert_eq!(decoded.target_id, "b");
        assert_eq!(decoded.kind, AssociationKind::Updates);
    }

    #[test]
    fn with_confidence_clamps() {
        let assoc =
            MemoryAssociation::new("a", "b", AssociationKind::RelatedTo).with_confidence(1.5);
        assert!((assoc.confidence - 1.0).abs() < f64::EPSILON);

        let assoc2 =
            MemoryAssociation::new("a", "b", AssociationKind::Contradicts).with_confidence(-0.5);
        assert!(assoc2.confidence.abs() < f64::EPSILON);
    }

    #[test]
    fn all_kinds_serialize() {
        for kind in [
            AssociationKind::RelatedTo,
            AssociationKind::Updates,
            AssociationKind::Contradicts,
            AssociationKind::CausedBy,
        ] {
            let json = serde_json::to_value(kind).unwrap();
            let decoded: AssociationKind = serde_json::from_value(json).unwrap();
            assert_eq!(decoded, kind);
        }
    }
}
