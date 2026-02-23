#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::Arc;

use super::types::{Artifact, Axis, AxisScores, TasteContext, TextFormat};
use crate::core::providers::{Provider, scrub_secret_patterns};
use async_trait::async_trait;

/// Result of critiquing an artifact (axis scores, raw response, confidence).
pub struct CritiqueResult {
    pub axis_scores: AxisScores,
    pub raw_response: String,
    pub confidence: f64,
}

#[async_trait]
pub(crate) trait UniversalCritic: Send + Sync {
    async fn critique(
        &self,
        artifact: &Artifact,
        ctx: &TasteContext,
    ) -> anyhow::Result<CritiqueResult>;
}

pub struct LlmCritic {
    provider: Arc<dyn Provider>,
    model: String,
}

impl LlmCritic {
    pub fn new(provider: Arc<dyn Provider>, model: String) -> Self {
        Self { provider, model }
    }

    pub(crate) fn build_system_prompt() -> String {
        [
            "You are a strict aesthetic critic scoring exactly three axes.",
            "Return JSON only with keys: coherence, hierarchy, intentionality, rationale.",
            "Scoring rubric (all scores must be in [0.0, 1.0]):",
            "- Coherence: Elements belong to the same worldview/style. Score 0.0=completely fragmented, 1.0=seamless stylistic unity.",
            "- Hierarchy: Primary focus is instantly identifiable. Score 0.0=everything equal weight, 1.0=clear visual/logical hierarchy.",
            "- Intentionality: Deliberate choices visible vs accidental assembly. Score 0.0=generic template, 1.0=every element purposefully chosen.",
            "Output example:",
            r#"{"coherence":0.0,"hierarchy":0.0,"intentionality":0.0,"rationale":"brief reason"}"#,
            "Do not include markdown fences or extra commentary.",
        ]
        .join("\n")
    }

    pub(crate) fn parse_critique_response(response: &str) -> CritiqueResult {
        fn build_scores(coherence: f64, hierarchy: f64, intentionality: f64) -> AxisScores {
            let mut axis_scores: AxisScores = BTreeMap::new();
            axis_scores.insert(Axis::Coherence, coherence.clamp(0.0, 1.0));
            axis_scores.insert(Axis::Hierarchy, hierarchy.clamp(0.0, 1.0));
            axis_scores.insert(Axis::Intentionality, intentionality.clamp(0.0, 1.0));
            axis_scores
        }

        fn zero_result(raw_response: &str) -> CritiqueResult {
            CritiqueResult {
                axis_scores: build_scores(0.0, 0.0, 0.0),
                raw_response: raw_response.to_string(),
                confidence: 0.7,
            }
        }

        let parsed = serde_json::from_str::<serde_json::Value>(response).or_else(|_| {
            let start = response.find('{');
            let end = response.rfind('}');
            match (start, end) {
                (Some(start), Some(end)) if start < end => {
                    serde_json::from_str::<serde_json::Value>(&response[start..=end])
                }
                _ => Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "no JSON object found",
                ))),
            }
        });

        let Ok(value) = parsed else {
            tracing::warn!("LlmCritic: failed to parse critique JSON; using zero-score fallback");
            return zero_result(response);
        };

        let Some(coherence) = value.get("coherence").and_then(serde_json::Value::as_f64) else {
            tracing::warn!("LlmCritic: missing coherence score; using zero-score fallback");
            return zero_result(response);
        };
        let Some(hierarchy) = value.get("hierarchy").and_then(serde_json::Value::as_f64) else {
            tracing::warn!("LlmCritic: missing hierarchy score; using zero-score fallback");
            return zero_result(response);
        };
        let Some(intentionality) = value
            .get("intentionality")
            .and_then(serde_json::Value::as_f64)
        else {
            tracing::warn!("LlmCritic: missing intentionality score; using zero-score fallback");
            return zero_result(response);
        };

        CritiqueResult {
            axis_scores: build_scores(coherence, hierarchy, intentionality),
            raw_response: response.to_string(),
            confidence: 0.7,
        }
    }

    fn format_artifact(artifact: &Artifact) -> String {
        match artifact {
            Artifact::Text { content, format } => {
                let format_label = match format {
                    Some(TextFormat::Plain) => "plain",
                    Some(TextFormat::Markdown) => "markdown",
                    Some(TextFormat::Html) => "html",
                    None => "unspecified",
                };
                format!("artifact_kind: text\nformat: {format_label}\ncontent:\n{content}")
            }
            Artifact::Ui {
                description,
                metadata,
            } => {
                let metadata_text = metadata
                    .as_ref()
                    .map_or_else(|| "null".to_string(), serde_json::Value::to_string);
                format!(
                    "artifact_kind: ui\ndescription:\n{description}\nmetadata:\n{metadata_text}"
                )
            }
        }
    }

    fn build_user_message(artifact: &Artifact, ctx: &TasteContext) -> String {
        let context_json = serde_json::to_string(ctx).unwrap_or_else(|_| "{}".to_string());
        format!(
            "Evaluate this artifact on Coherence, Hierarchy, and Intentionality only.\n\nContext:\n{context_json}\n\nArtifact:\n{}",
            Self::format_artifact(artifact)
        )
    }
}

#[async_trait]
impl UniversalCritic for LlmCritic {
    async fn critique(
        &self,
        artifact: &Artifact,
        ctx: &TasteContext,
    ) -> anyhow::Result<CritiqueResult> {
        let system_prompt = scrub_secret_patterns(&Self::build_system_prompt()).into_owned();
        let user_message =
            scrub_secret_patterns(&Self::build_user_message(artifact, ctx)).into_owned();

        let response = self
            .provider
            .chat_with_system(Some(&system_prompt), &user_message, &self.model, 0.0)
            .await?;

        let scrubbed_response = scrub_secret_patterns(&response).into_owned();
        let mut critique = Self::parse_critique_response(&scrubbed_response);
        critique.raw_response = scrubbed_response;
        Ok(critique)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_contains_rubric_definitions() {
        let prompt = LlmCritic::build_system_prompt();
        assert!(
            prompt.contains("Coherence") || prompt.contains("coherence"),
            "Prompt must contain Coherence rubric"
        );
        assert!(
            prompt.contains("Hierarchy") || prompt.contains("hierarchy"),
            "Prompt must contain Hierarchy rubric"
        );
        assert!(
            prompt.contains("Intentionality") || prompt.contains("intentionality"),
            "Prompt must contain Intentionality rubric"
        );
        assert!(
            prompt.contains("0.0") && prompt.contains("1.0"),
            "Prompt must include score range"
        );
    }

    #[test]
    fn test_parse_valid_json_response() {
        let json =
            r#"{"coherence": 0.8, "hierarchy": 0.6, "intentionality": 0.9, "rationale": "good"}"#;
        let cr = LlmCritic::parse_critique_response(json);
        assert!((cr.axis_scores[&Axis::Coherence] - 0.8).abs() < 0.001);
        assert!((cr.axis_scores[&Axis::Hierarchy] - 0.6).abs() < 0.001);
        assert!((cr.axis_scores[&Axis::Intentionality] - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_parse_malformed_json_returns_fallback() {
        let bad_json = "this is not json at all";
        let cr = LlmCritic::parse_critique_response(bad_json);
        for score in cr.axis_scores.values() {
            assert!((*score - 0.0).abs() < f64::EPSILON);
            assert!(*score >= 0.0 && *score <= 1.0);
        }
    }

    #[test]
    fn test_scores_clamped_to_unit_interval() {
        let json = r#"{"coherence": 1.5, "hierarchy": -0.2, "intentionality": 0.5}"#;
        let cr = LlmCritic::parse_critique_response(json);
        for score in cr.axis_scores.values() {
            assert!(
                *score >= 0.0 && *score <= 1.0,
                "Score {} out of bounds",
                score
            );
        }
    }
}
