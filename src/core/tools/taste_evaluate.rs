use super::traits::{Tool, ToolResult};
use crate::core::taste::engine::TasteEngine;
use crate::core::taste::types::{Artifact, TasteContext, TextFormat};
use crate::core::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;


pub struct TasteEvaluateTool {
    engine: Arc<dyn TasteEngine>,
}

impl TasteEvaluateTool {
    pub fn new(engine: Arc<dyn TasteEngine>) -> Self {
        Self { engine }
    }

    fn parse_artifact(artifact: &serde_json::Value) -> anyhow::Result<Artifact> {
        let kind = artifact
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'kind' in artifact"))?;

        let content = artifact
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'content' in artifact"))?;

        match kind {
            "text" => {
                let format = artifact
                    .get("format")
                    .and_then(|v| v.as_str())
                    .map(|f| serde_json::from_value::<TextFormat>(json!(f)))
                    .transpose()?;
                Ok(Artifact::Text {
                    content: content.to_string(),
                    format,
                })
            }
            "ui" => {
                let metadata = artifact.get("metadata").cloned();
                Ok(Artifact::Ui {
                    description: content.to_string(),
                    metadata,
                })
            }
            other => anyhow::bail!("Unsupported artifact kind: {other}"),
        }
    }

    fn parse_context(context: Option<&serde_json::Value>) -> anyhow::Result<TasteContext> {
        match context {
            Some(v) => Ok(serde_json::from_value(v.clone())?),
            None => Ok(TasteContext::default()),
        }
    }
}

#[async_trait]
impl Tool for TasteEvaluateTool {
    fn name(&self) -> &str {
        "taste_evaluate"
    }

    fn description(&self) -> &str {
        "Evaluate an artifact's aesthetic quality using the taste engine."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "artifact": {
                    "type": "object",
                    "description": "The artifact to evaluate",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "The artifact content (text content or UI description)"
                        },
                        "format": {
                            "type": "string",
                            "description": "Text format: plain, markdown, or html (text kind only)"
                        },
                        "kind": {
                            "type": "string",
                            "enum": ["text", "ui"],
                            "description": "Artifact kind"
                        }
                    },
                    "required": ["content", "kind"]
                },
                "context": {
                    "type": "object",
                    "description": "Evaluation context",
                    "properties": {
                        "domain": {
                            "type": "string",
                            "description": "Domain: text, ui, or general"
                        },
                        "genre": {
                            "type": "string",
                            "description": "Genre of the artifact"
                        },
                        "purpose": {
                            "type": "string",
                            "description": "Purpose of the artifact"
                        }
                    }
                }
            },
            "required": ["artifact"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let Some(artifact_value) = args.get("artifact") else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Missing 'artifact' parameter".to_string()),
                attachments: vec![],
            });
        };

        let artifact = match Self::parse_artifact(artifact_value) {
            Ok(a) => a,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                    attachments: vec![],
                });
            }
        };

        let taste_ctx = match Self::parse_context(args.get("context")) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(e.to_string()),
                    attachments: vec![],
                });
            }
        };

        match self.engine.evaluate(&artifact, &taste_ctx).await {
            Ok(report) => match serde_json::to_string(&report) {
                Ok(json_string) => Ok(ToolResult {
                    success: true,
                    output: json_string,
                    error: None,
                    attachments: vec![],
                }),
                Err(e) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to serialize report: {e}")),
                    attachments: vec![],
                }),
            },
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
        Axis, Domain, PairComparison, Priority, Suggestion, TasteReport,
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
    async fn evaluate_text_artifact() {
        let tool = TasteEvaluateTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "artifact": {
                        "kind": "text",
                        "content": "Hello world"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        let parsed: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(parsed.get("axis").is_some());
        assert!(parsed.get("suggestions").is_some());
    }

    #[tokio::test]
    async fn evaluate_ui_artifact() {
        let tool = TasteEvaluateTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "artifact": {
                        "kind": "ui",
                        "content": "A dashboard with sidebar navigation"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
        let parsed: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert!(parsed.get("axis").is_some());
        assert!(parsed.get("suggestions").is_some());
    }

    #[tokio::test]
    async fn evaluate_with_context() {
        let tool = TasteEvaluateTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "artifact": {
                        "kind": "text",
                        "content": "Test content",
                        "format": "markdown"
                    },
                    "context": {
                        "domain": "text",
                        "genre": "technical",
                        "purpose": "documentation"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn evaluate_missing_artifact() {
        let tool = TasteEvaluateTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool.execute(json!({}), &ctx).await.unwrap();
        assert!(!result.success);
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Missing 'artifact'"));
    }

    #[tokio::test]
    async fn evaluate_missing_kind() {
        let tool = TasteEvaluateTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "artifact": {
                        "content": "hello"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("kind"));
    }

    #[tokio::test]
    async fn evaluate_unsupported_kind() {
        let tool = TasteEvaluateTool::new(mock_engine());
        let ctx = ExecutionContext::test_default(Arc::new(SecurityPolicy::default()));
        let result = tool
            .execute(
                json!({
                    "artifact": {
                        "kind": "audio",
                        "content": "something"
                    }
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Unsupported artifact kind"));
    }

    #[test]
    fn name_and_schema() {
        let tool = TasteEvaluateTool::new(mock_engine());
        assert_eq!(tool.name(), "taste_evaluate");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["artifact"].is_object());
        assert!(schema["properties"]["context"].is_object());
    }
}
