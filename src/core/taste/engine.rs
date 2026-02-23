#![allow(dead_code)]

use crate::config::TasteConfig;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use super::adapter::DomainAdapter;
use super::critic::UniversalCritic;
use super::learner::TasteLearner;
use super::store::TasteStore;
use super::types::{Artifact, Domain, PairComparison, TasteContext, TasteReport};

/// Trait for evaluating artifacts and comparing preferences.
#[async_trait]
pub trait TasteEngine: Send + Sync {
    async fn evaluate(
        &self,
        artifact: &Artifact,
        ctx: &TasteContext,
    ) -> anyhow::Result<TasteReport>;

    async fn compare(&self, comparison: &PairComparison) -> anyhow::Result<()>;

    fn enabled(&self) -> bool;
}

pub struct DefaultTasteEngine {
    pub config: TasteConfig,
    pub(crate) critic: Arc<dyn UniversalCritic>,
    pub(crate) adapters: HashMap<Domain, Arc<dyn DomainAdapter>>,
    pub(crate) store: Option<Arc<dyn TasteStore>>,
    pub(crate) learner: Option<Arc<dyn TasteLearner>>,
}

#[async_trait]
impl TasteEngine for DefaultTasteEngine {
    async fn evaluate(
        &self,
        _artifact: &Artifact,
        _ctx: &TasteContext,
    ) -> anyhow::Result<TasteReport> {
        anyhow::bail!("DefaultTasteEngine::evaluate: not yet wired (T9)")
    }

    async fn compare(&self, _comparison: &PairComparison) -> anyhow::Result<()> {
        anyhow::bail!("DefaultTasteEngine::compare: not yet wired (T14)")
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Creates a taste engine instance from configuration.
pub fn create_taste_engine(_config: &TasteConfig) -> anyhow::Result<Arc<dyn TasteEngine>> {
    anyhow::bail!("create_taste_engine: full wiring in T9 (Wave 3)")
}
