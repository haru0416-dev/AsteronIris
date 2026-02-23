use crate::config::TasteConfig;
use crate::core::providers::Provider;
use rusqlite::Connection;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use super::adapter::{DomainAdapter, TextAdapter, UiAdapter};
use super::critic::{LlmCritic, UniversalCritic};
use super::learner::{BradleyTerryLearner, TasteLearner};
use super::store::{SqliteTasteStore, TasteStore};
use super::types::{Artifact, Domain, PairComparison, TasteContext, TasteReport, Winner};

/// Trait for evaluating artifacts and comparing preferences.
pub trait TasteEngine: Send + Sync {
    fn evaluate<'a>(
        &'a self,
        artifact: &'a Artifact,
        ctx: &'a TasteContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<TasteReport>> + Send + 'a>>;

    fn compare<'a>(
        &'a self,
        comparison: &'a PairComparison,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

    fn enabled(&self) -> bool;
}

pub struct DefaultTasteEngine {
    pub config: TasteConfig,
    pub(crate) critic: Arc<dyn UniversalCritic>,
    pub(crate) adapters: HashMap<Domain, Arc<dyn DomainAdapter>>,
    pub(crate) store: Option<Arc<dyn TasteStore>>,
    pub(crate) learner: Option<Arc<Mutex<BradleyTerryLearner>>>,
}

impl TasteEngine for DefaultTasteEngine {
    fn evaluate<'a>(
        &'a self,
        artifact: &'a Artifact,
        ctx: &'a TasteContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<TasteReport>> + Send + 'a>> {
        Box::pin(async move {
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
        })
    }

    fn compare<'a>(
        &'a self,
        comparison: &'a PairComparison,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
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
        })
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Creates a taste engine instance from configuration.
pub fn create_taste_engine(
    config: &TasteConfig,
    provider: Arc<dyn Provider>,
    model: String,
) -> anyhow::Result<Arc<dyn TasteEngine>> {
    let critic = LlmCritic::new(provider, model);

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
