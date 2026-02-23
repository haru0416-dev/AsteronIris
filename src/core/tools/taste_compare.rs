use super::traits::{Tool, ToolResult};
use crate::core::taste::engine::TasteEngine;
use crate::core::taste::types::{Domain, PairComparison, TasteContext, Winner};
use crate::core::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct TasteCompareTool {
    engine: Arc<dyn TasteEngine>,
}

impl TasteCompareTool {
    pub fn new(engine: Arc<dyn TasteEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl Tool for TasteCompareTool {
    fn name(&self) -> &str {
        "taste_compare"
    }

    fn description(&self) -> &str {
        "Record a pairwise preference comparison between two artifacts."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "left_id": {
                    "type": "string",
                    "description": "Identifier of the left artifact"
                },
                "right_id": {
                    "type": "string",
                    "description": "Identifier of the right artifact"
                },
                "winner": {
                    "type": "string",
                    "enum": ["left", "right", "tie", "abstain"],
                    "description": "Which artifact won the comparison"
                },
                "domain": {
                    "type": "string",
                    "description": "Domain: text, ui, or general (default: general)"
                },
                "rationale": {
                    "type": "string",
                    "description": "Optional rationale for the preference"
                }
            },
            "required": ["left_id", "right_id", "winner"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let Some(left_id) = args.get("left_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'left_id' parameter".to_string()),
                attachments: vec![],
            });
        };

        let Some(right_id) = args.get("right_id").and_then(|v| v.as_str()) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'right_id' parameter".to_string()),
                attachments: vec![],
            });
        };

        let Some(winner_str) = args.get("winner").and_then(|v| v.as_str()) else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'winner' parameter".to_string()),
                attachments: vec![],
            });
        };

        let winner: Winner = match serde_json::from_value(serde_json::Value::String(
            winner_str.to_string(),
        )) {
            Ok(w) => w,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                    attachments: vec![],
                });
            }
        };

        let domain_str = args
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("general")
            .to_string();

        let domain: Domain = match serde_json::from_value(serde_json::Value::String(domain_str)) {
            Ok(d) => d,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                    attachments: vec![],
                });
            }
        };

        let rationale = args.get("rationale").and_then(|v| v.as_str()).map(String::from);

        #[allow(clippy::cast_possible_truncation)]
        let created_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let comparison = PairComparison {
            domain,
            ctx: TasteContext::default(),
            left_id: left_id.to_string(),
            right_id: right_id.to_string(),
            winner: winner.clone(),
            rationale,
            created_at_ms,
        };

        match self.engine.compare(&comparison).await {
            Ok(()) => Ok(ToolResult {
                success: true,
                output: json!({
                    "status": "comparison_recorded",
                    "left_id": left_id,
                    "right_id": right_id,
                    "winner": winner_str
                })
                .to_string(),
                error: None,
                attachments: vec![],
            }),
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
                attachments: vec![],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::taste::types::{
        Artifact, Axis, Domain, PairComparison, Priority, Suggestion, TasteContext, TasteReport,
    };
    use crate::security::SecurityPolicy;
    use std::collections::BTreeMap;

    struct MockTasteEngine;

    #[async_trait]
    impl TasteEngine for MockTasteEngine {
        async fn evaluate(
            &self,
            _artifact: &Artifact,
            _ctx: &TasteContext,
        ) -> anyhow::Result<TasteReport> {
            let mut axis = BTreeMap::new();
            axis.insert(Axis::Coherence, 0.8);
            axis.insert(Axis::Hierarchy, 0.7);
            axis.insert(Axis::Intentionality, 0.9);
            Ok(TasteReport {
                axis,
                domain: Domain::Text,
                suggestions: vec![Suggestion::General {
                    title: "Improve structure".into(),
                    rationale: "Would benefit from clearer sections".into(),
                    priority: Priority::Medium,
                }],
                raw_critique: None,
            })
        }

        async fn compare(&self, _comparison: &PairComparison) -> anyhow::Result<()> {
            Ok(())
        }

        fn enabled(&self) -> bool {
            true
        }
    }

    fn mock_engine() -> Arc<dyn TasteEngine> {
        Arc::new(MockTasteEngine)
    }

    #[tokio::test]
    async fn compare_valid_args() {
        let tool = TasteCompareTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "left_id": "artifact_a",
                    "right_id": "artifact_b",
                    "winner": "left"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        let parsed: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(parsed["status"], "comparison_recorded");
        assert_eq!(parsed["left_id"], "artifact_a");
        assert_eq!(parsed["right_id"], "artifact_b");
        assert_eq!(parsed["winner"], "left");
    }

    #[tokio::test]
    async fn compare_with_optional_fields() {
        let tool = TasteCompareTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "left_id": "a",
                    "right_id": "b",
                    "winner": "tie",
                    "domain": "text",
                    "rationale": "Both are equally good"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn compare_missing_winner() {
        let tool = TasteCompareTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "left_id": "a",
                    "right_id": "b"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("winner"));
    }

    #[tokio::test]
    async fn compare_missing_left_id() {
        let tool = TasteCompareTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "right_id": "b",
                    "winner": "left"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("left_id"));
    }

    #[tokio::test]
    async fn compare_invalid_winner() {
        let tool = TasteCompareTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "left_id": "a",
                    "right_id": "b",
                    "winner": "invalid"
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn name_and_schema() {
        let tool = TasteCompareTool::new(mock_engine());
        assert_eq!(tool.name(), "taste_compare");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["left_id"].is_object());
        assert!(schema["properties"]["right_id"].is_object());
        assert!(schema["properties"]["winner"].is_object());
        assert!(schema["properties"]["domain"].is_object());
        assert!(schema["properties"]["rationale"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 3);
    }
}
