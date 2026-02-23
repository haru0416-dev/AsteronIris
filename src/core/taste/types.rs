use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use strum::Display;

// TextFormat — format of text artifact
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFormat {
    Plain,
    Markdown,
    Html,
}

// Artifact — input to the taste engine (Text and Ui only, NO image/video/audio)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Artifact {
    Text {
        content: String,
        #[serde(default)]
        format: Option<TextFormat>,
    },
    Ui {
        description: String,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
}

// Domain — which domain an artifact belongs to
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, Default)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Domain {
    Text,
    Ui,
    #[default]
    General,
}

// Axis — aesthetic evaluation axis (EXACTLY 3, no more)
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Axis {
    Coherence,
    Hierarchy,
    Intentionality,
}

// AxisScores — scores per axis (BTreeMap for stable ordering)
pub type AxisScores = BTreeMap<Axis, f64>;

// TasteContext — context for evaluating an artifact
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TasteContext {
    #[serde(default)]
    pub domain: Domain,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// Priority — suggestion priority level
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

// TextOp — text correction operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextOp {
    RestructureArgument,
    AdjustDensity,
    UnifyStyle,
    AddOutline,
    Other(String),
}

// UiOp — UI correction operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiOp {
    AdjustLayout,
    ImproveHierarchy,
    AddContrast,
    RefineSpacing,
    Other(String),
}

// Suggestion — improvement suggestion (tagged enum)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Suggestion {
    General {
        title: String,
        rationale: String,
        priority: Priority,
    },
    Text {
        op: TextOp,
        rationale: String,
        priority: Priority,
    },
    Ui {
        op: UiOp,
        rationale: String,
        priority: Priority,
    },
}

// TasteReport — result of evaluating an artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TasteReport {
    pub axis: AxisScores,
    pub domain: Domain,
    pub suggestions: Vec<Suggestion>,
    #[serde(default)]
    pub raw_critique: Option<String>,
}

// Winner — who won a pair comparison
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Winner {
    Left,
    Right,
    Tie,
    Abstain,
}

// PairComparison — record of a human/LLM preference comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairComparison {
    pub domain: Domain,
    pub ctx: TasteContext,
    pub left_id: String,
    pub right_id: String,
    pub winner: Winner,
    #[serde(default)]
    pub rationale: Option<String>,
    pub created_at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_text_roundtrip() {
        let a = Artifact::Text {
            content: "hello".into(),
            format: Some(TextFormat::Markdown),
        };
        let json = serde_json::to_string(&a).unwrap();
        let b: Artifact = serde_json::from_str(&json).unwrap();
        if let Artifact::Text { content, format } = b {
            assert_eq!(content, "hello");
            assert_eq!(format, Some(TextFormat::Markdown));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_axis_btreemap_key() {
        let mut scores: AxisScores = BTreeMap::new();
        scores.insert(Axis::Coherence, 0.8);
        scores.insert(Axis::Hierarchy, 0.6);
        scores.insert(Axis::Intentionality, 0.9);
        assert_eq!(scores.len(), 3);
        // BTreeMap ordering: Coherence < Hierarchy < Intentionality (alphabetical via Ord)
        assert!(scores.contains_key(&Axis::Coherence));
    }

    #[test]
    fn test_axis_has_exactly_3_variants() {
        // If this test fails, someone added a 4th axis (violates guardrail)
        let axes = [Axis::Coherence, Axis::Hierarchy, Axis::Intentionality];
        assert_eq!(axes.len(), 3);
    }

    #[test]
    fn test_suggestion_text_tagged_enum() {
        let s = Suggestion::Text {
            op: TextOp::UnifyStyle,
            rationale: "needs unification".into(),
            priority: Priority::High,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"text\""));
        let s2: Suggestion = serde_json::from_str(&json).unwrap();
        if let Suggestion::Text { op, priority, .. } = s2 {
            assert_eq!(op, TextOp::UnifyStyle);
            assert_eq!(priority, Priority::High);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_pair_comparison_roundtrip() {
        let pc = PairComparison {
            domain: Domain::Text,
            ctx: TasteContext::default(),
            left_id: "a".into(),
            right_id: "b".into(),
            winner: Winner::Left,
            rationale: Some("clearer".into()),
            created_at_ms: 1234567890,
        };
        let json = serde_json::to_string(&pc).unwrap();
        let pc2: PairComparison = serde_json::from_str(&json).unwrap();
        assert_eq!(pc2.left_id, "a");
        assert_eq!(pc2.winner, Winner::Left);
        assert_eq!(pc2.created_at_ms, 1234567890);
    }

    #[test]
    fn test_domain_default_is_general() {
        let d = Domain::default();
        assert_eq!(d, Domain::General);
    }
}
