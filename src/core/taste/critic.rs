#![allow(dead_code)]

use super::types::{Artifact, AxisScores, TasteContext};
use async_trait::async_trait;

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
