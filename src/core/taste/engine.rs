use crate::config::TasteConfig;
use crate::core::providers::create_provider;
use async_trait::async_trait;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::adapter::{DomainAdapter, TextAdapter, UiAdapter};
use super::critic::{LlmCritic, UniversalCritic};
use super::learner::{BradleyTerryLearner, TasteLearner};
use super::store::{SqliteTasteStore, TasteStore};
use super::types::{Artifact, Domain, PairComparison, TasteContext, TasteReport, Winner};

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
    pub(crate) learner: Option<Arc<Mutex<BradleyTerryLearner>>>,
}

#[async_trait]
impl TasteEngine for DefaultTasteEngine {
    async fn evaluate(
        &self,
        artifact: &Artifact,
        ctx: &TasteContext,
    ) -> anyhow::Result<TasteReport> {
        let critique = self.critic.critique(artifact, ctx).await?;

        let domain = match artifact {
            Artifact::Text { .. } => Domain::Text,
            Artifact::Ui { .. } => Domain::Ui,
        };

        let suggestions = self
            .adapters
            .get(&domain)
            .map(|adapter| adapter.suggest(&critique, ctx))
            .unwrap_or_default();

        Ok(TasteReport {
            axis: critique.axis_scores,
            domain,
            suggestions,
            raw_critique: Some(critique.raw_response),
        })
    }

    async fn compare(&self, comparison: &PairComparison) -> anyhow::Result<()> {
        if let Some(store) = &self.store {
            store.save_comparison(comparison).await?;
        }

        if let Some(learner) = &self.learner {
            let mut l = learner
                .lock()
                .map_err(|e| anyhow::anyhow!("learner lock poisoned: {e}"))?;

            let outcome = match comparison.winner {
                Winner::Left => 1.0,
                Winner::Right => 0.0,
                Winner::Tie => 0.5,
                Winner::Abstain => return Ok(()),
            };

            l.update(&comparison.left_id, &comparison.right_id, outcome);
        }

        Ok(())
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Creates a taste engine instance from configuration.
pub fn create_taste_engine(config: &TasteConfig) -> anyhow::Result<Arc<dyn TasteEngine>> {
    let provider = create_provider("synthetic", None).map_err(|e| {
        anyhow::anyhow!("failed to create synthetic provider for taste engine: {e}")
    })?;

    let critic = LlmCritic::new(Arc::from(provider), "claude-3-haiku-20240307".to_string());

    let mut adapters: HashMap<Domain, Arc<dyn DomainAdapter>> = HashMap::new();
    if config.text_enabled {
        adapters.insert(Domain::Text, Arc::new(TextAdapter));
    }
    if config.ui_enabled {
        adapters.insert(Domain::Ui, Arc::new(UiAdapter));
    }

    let conn = Connection::open_in_memory()?;
    let store = SqliteTasteStore::new(conn)?;

    let learner = BradleyTerryLearner::new();

    let engine = DefaultTasteEngine {
        config: config.clone(),
        critic: Arc::new(critic),
        adapters,
        store: Some(Arc::new(store)),
        learner: Some(Arc::new(Mutex::new(learner))),
    };

    Ok(Arc::new(engine))
}
