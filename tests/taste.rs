#![cfg(feature = "taste")]

use std::collections::BTreeMap;

use asteroniris::config::TasteConfig;
use asteroniris::core::taste::{
    Artifact, Axis, AxisScores, Domain, PairComparison, Priority, Suggestion, TasteContext,
    TasteReport, TextFormat, TextOp, UiOp, Winner,
};

fn all_axes_scores(coherence: f64, hierarchy: f64, intentionality: f64) -> AxisScores {
    let mut scores = BTreeMap::new();
    scores.insert(Axis::Coherence, coherence);
    scores.insert(Axis::Hierarchy, hierarchy);
    scores.insert(Axis::Intentionality, intentionality);
    scores
}

mod type_roundtrips {
    use super::*;

    #[test]
    fn artifact_text_roundtrip() {
        let artifact = Artifact::Text {
            content: "hello world".into(),
            format: Some(TextFormat::Markdown),
        };
        let json = serde_json::to_string(&artifact).unwrap();
        let recovered: Artifact = serde_json::from_str(&json).unwrap();
        if let Artifact::Text { content, format } = recovered {
            assert_eq!(content, "hello world");
            assert_eq!(format, Some(TextFormat::Markdown));
        } else {
            panic!("expected Artifact::Text");
        }
    }

    #[test]
    fn artifact_ui_roundtrip() {
        let artifact = Artifact::Ui {
            description: "a dashboard".into(),
            metadata: Some(serde_json::json!({"columns": 3})),
        };
        let json = serde_json::to_string(&artifact).unwrap();
        let recovered: Artifact = serde_json::from_str(&json).unwrap();
        if let Artifact::Ui {
            description,
            metadata,
        } = recovered
        {
            assert_eq!(description, "a dashboard");
            assert!(metadata.is_some());
        } else {
            panic!("expected Artifact::Ui");
        }
    }

    #[test]
    fn artifact_text_tagged_kind() {
        let artifact = Artifact::Text {
            content: "x".into(),
            format: None,
        };
        let json = serde_json::to_string(&artifact).unwrap();
        assert!(json.contains("\"kind\":\"text\""));
    }

    #[test]
    fn artifact_ui_tagged_kind() {
        let artifact = Artifact::Ui {
            description: "x".into(),
            metadata: None,
        };
        let json = serde_json::to_string(&artifact).unwrap();
        assert!(json.contains("\"kind\":\"ui\""));
    }

    #[test]
    fn taste_report_roundtrip() {
        let report = TasteReport {
            axis: all_axes_scores(0.8, 0.6, 0.9),
            domain: Domain::Text,
            suggestions: vec![Suggestion::General {
                title: "improve clarity".into(),
                rationale: "too dense".into(),
                priority: Priority::Medium,
            }],
            raw_critique: Some("good overall".into()),
        };
        let json = serde_json::to_string(&report).unwrap();
        let recovered: TasteReport = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.axis.len(), 3);
        assert_eq!(recovered.domain, Domain::Text);
        assert_eq!(recovered.suggestions.len(), 1);
        assert_eq!(recovered.raw_critique.as_deref(), Some("good overall"));
    }

    #[test]
    fn taste_report_empty_suggestions() {
        let report = TasteReport {
            axis: all_axes_scores(1.0, 1.0, 1.0),
            domain: Domain::Ui,
            suggestions: vec![],
            raw_critique: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let recovered: TasteReport = serde_json::from_str(&json).unwrap();
        assert!(recovered.suggestions.is_empty());
        assert!(recovered.raw_critique.is_none());
    }

    #[test]
    fn pair_comparison_roundtrip() {
        let pc = PairComparison {
            domain: Domain::Text,
            ctx: TasteContext::default(),
            left_id: "artifact_a".into(),
            right_id: "artifact_b".into(),
            winner: Winner::Left,
            rationale: Some("clearer argument structure".into()),
            created_at_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&pc).unwrap();
        let recovered: PairComparison = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.left_id, "artifact_a");
        assert_eq!(recovered.right_id, "artifact_b");
        assert_eq!(recovered.winner, Winner::Left);
        assert_eq!(recovered.created_at_ms, 1_700_000_000_000);
    }

    #[test]
    fn pair_comparison_all_winners() {
        for winner in [Winner::Left, Winner::Right, Winner::Tie, Winner::Abstain] {
            let pc = PairComparison {
                domain: Domain::General,
                ctx: TasteContext::default(),
                left_id: "l".into(),
                right_id: "r".into(),
                winner: winner.clone(),
                rationale: None,
                created_at_ms: 0,
            };
            let json = serde_json::to_string(&pc).unwrap();
            let recovered: PairComparison = serde_json::from_str(&json).unwrap();
            assert_eq!(recovered.winner, winner);
        }
    }

    #[test]
    fn suggestion_text_roundtrip() {
        let s = Suggestion::Text {
            op: TextOp::UnifyStyle,
            rationale: "inconsistent tone".into(),
            priority: Priority::High,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"text\""));
        let recovered: Suggestion = serde_json::from_str(&json).unwrap();
        if let Suggestion::Text { op, priority, .. } = recovered {
            assert_eq!(op, TextOp::UnifyStyle);
            assert_eq!(priority, Priority::High);
        } else {
            panic!("expected Suggestion::Text");
        }
    }

    #[test]
    fn suggestion_ui_roundtrip() {
        let s = Suggestion::Ui {
            op: UiOp::ImproveHierarchy,
            rationale: "flat layout".into(),
            priority: Priority::Low,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"ui\""));
        let recovered: Suggestion = serde_json::from_str(&json).unwrap();
        if let Suggestion::Ui { op, priority, .. } = recovered {
            assert_eq!(op, UiOp::ImproveHierarchy);
            assert_eq!(priority, Priority::Low);
        } else {
            panic!("expected Suggestion::Ui");
        }
    }

    #[test]
    fn suggestion_general_roundtrip() {
        let s = Suggestion::General {
            title: "add introduction".into(),
            rationale: "missing context".into(),
            priority: Priority::Medium,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"general\""));
        let recovered: Suggestion = serde_json::from_str(&json).unwrap();
        if let Suggestion::General {
            title, priority, ..
        } = recovered
        {
            assert_eq!(title, "add introduction");
            assert_eq!(priority, Priority::Medium);
        } else {
            panic!("expected Suggestion::General");
        }
    }

    #[test]
    fn taste_context_roundtrip() {
        let ctx = TasteContext {
            domain: Domain::Text,
            genre: Some("technical".into()),
            purpose: Some("documentation".into()),
            audience: Some("developers".into()),
            constraints: vec!["max 500 words".into()],
            extra: serde_json::Map::new(),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let recovered: TasteContext = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.domain, Domain::Text);
        assert_eq!(recovered.genre.as_deref(), Some("technical"));
        assert_eq!(recovered.constraints.len(), 1);
    }

    #[test]
    fn taste_context_default_deserializes_from_empty_json() {
        let ctx: TasteContext = serde_json::from_str("{}").unwrap();
        assert_eq!(ctx.domain, Domain::General);
        assert!(ctx.genre.is_none());
        assert!(ctx.constraints.is_empty());
    }

    #[test]
    fn axis_scores_stable_ordering() {
        let scores = all_axes_scores(0.1, 0.2, 0.3);
        let keys: Vec<&Axis> = scores.keys().collect();

        assert_eq!(*keys[0], Axis::Coherence);
        assert_eq!(*keys[1], Axis::Hierarchy);
        assert_eq!(*keys[2], Axis::Intentionality);
    }
}

mod config_defaults {
    use super::*;

    #[test]
    fn default_config_disabled() {
        let cfg = TasteConfig::default();
        assert!(!cfg.enabled, "taste should be disabled by default");
    }

    #[test]
    fn default_config_backend() {
        let cfg = TasteConfig::default();
        assert_eq!(cfg.backend, "llm");
    }

    #[test]
    fn default_config_has_three_axes() {
        let cfg = TasteConfig::default();
        assert_eq!(cfg.axes.len(), 3);
        assert!(cfg.axes.contains(&"coherence".to_string()));
        assert!(cfg.axes.contains(&"hierarchy".to_string()));
        assert!(cfg.axes.contains(&"intentionality".to_string()));
    }

    #[test]
    fn default_config_domains_enabled() {
        let cfg = TasteConfig::default();
        assert!(cfg.text_enabled);
        assert!(cfg.ui_enabled);
    }

    #[test]
    fn config_json_roundtrip() {
        let cfg = TasteConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let recovered: TasteConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.backend, cfg.backend);
        assert_eq!(recovered.axes, cfg.axes);
        assert_eq!(recovered.enabled, cfg.enabled);
    }
}

mod domain_tests {
    use super::*;

    #[test]
    fn domain_default_is_general() {
        assert_eq!(Domain::default(), Domain::General);
    }

    #[test]
    fn domain_display_snake_case() {
        assert_eq!(Domain::Text.to_string(), "text");
        assert_eq!(Domain::Ui.to_string(), "ui");
        assert_eq!(Domain::General.to_string(), "general");
    }

    #[test]
    fn axis_display_snake_case() {
        assert_eq!(Axis::Coherence.to_string(), "coherence");
        assert_eq!(Axis::Hierarchy.to_string(), "hierarchy");
        assert_eq!(Axis::Intentionality.to_string(), "intentionality");
    }

    #[test]
    fn axis_exactly_three_variants() {
        let axes = [Axis::Coherence, Axis::Hierarchy, Axis::Intentionality];
        assert_eq!(axes.len(), 3);
    }
}
