use crate::core::memory::memory_types::{
    MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, RecallQuery, SignalTier,
    SourceKind,
};
use crate::core::memory::traits::{Memory, MemoryLayer};
use crate::runtime::observability::traits::{AutonomyLifecycleSignal, ObserverMetric};
use crate::runtime::observability::{NoopObserver, Observer};
use crate::security::writeback_guard::enforce_ingestion_write_policy;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use super::signal_envelope::SignalEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionResult {
    pub accepted: bool,
    pub slot_key: String,
    pub signal_tier: SignalTier,
    pub reason: Option<String>,
}

pub trait IngestionPipeline: Send + Sync {
    async fn ingest(&self, envelope: SignalEnvelope) -> anyhow::Result<IngestionResult>;

    async fn ingest_batch(
        &self,
        envelopes: Vec<SignalEnvelope>,
    ) -> anyhow::Result<Vec<IngestionResult>> {
        let mut results = Vec::with_capacity(envelopes.len());
        for envelope in envelopes {
            results.push(self.ingest(envelope).await?);
        }
        Ok(results)
    }
}

#[derive(Clone)]
pub struct SqliteIngestionPipeline {
    memory: Arc<dyn Memory>,
    semantic_dedup_cache: Arc<Mutex<HashSet<String>>>,
    observer: Arc<dyn Observer>,
}

impl SqliteIngestionPipeline {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self::new_with_observer(memory, Arc::new(NoopObserver))
    }

    pub fn new_with_observer(memory: Arc<dyn Memory>, observer: Arc<dyn Observer>) -> Self {
        Self {
            memory,
            semantic_dedup_cache: Arc::new(Mutex::new(HashSet::new())),
            observer,
        }
    }

    async fn is_source_ref_exact_duplicate(
        &self,
        envelope: &SignalEnvelope,
        slot_key: &str,
    ) -> anyhow::Result<bool> {
        let existing = self
            .memory
            .resolve_slot(&envelope.entity_id, slot_key)
            .await?;
        Ok(existing.is_some_and(|slot| slot.value == envelope.content))
    }

    async fn is_semantic_duplicate(
        &self,
        envelope: &SignalEnvelope,
        slot_key: &str,
    ) -> anyhow::Result<bool> {
        let source_kind_prefix = format!("external.{}.", envelope.source_kind_str());
        let semantic_candidates = self
            .memory
            .recall_scoped(RecallQuery::new(&envelope.entity_id, &envelope.content, 5))
            .await?;

        Ok(semantic_candidates.iter().any(|item| {
            item.slot_key != slot_key
                && item.slot_key.starts_with(&source_kind_prefix)
                && (item.value == envelope.content || item.score >= 0.95)
        }))
    }

    fn dedup_cache_contains(&self, semantic_key: &str) -> anyhow::Result<bool> {
        let cache = self
            .semantic_dedup_cache
            .lock()
            .map_err(|e| anyhow::anyhow!("semantic dedup cache lock poisoned: {e}"))?;
        Ok(cache.contains(semantic_key))
    }

    fn dedup_cache_insert(&self, semantic_key: String) -> anyhow::Result<()> {
        let mut cache = self
            .semantic_dedup_cache
            .lock()
            .map_err(|e| anyhow::anyhow!("semantic dedup cache lock poisoned: {e}"))?;
        cache.insert(semantic_key);
        Ok(())
    }
}

pub(super) fn semantic_dedup_key(
    entity_id: &str,
    source_kind: SourceKind,
    content: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(entity_id.as_bytes());
    hasher.update(b"::");
    hasher.update(source_kind.to_string().as_bytes());
    hasher.update(b"::");
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    format!("{digest:x}")
}

impl IngestionPipeline for SqliteIngestionPipeline {
    #[allow(clippy::too_many_lines)]
    async fn ingest(&self, envelope: SignalEnvelope) -> anyhow::Result<IngestionResult> {
        let envelope = envelope.normalize()?;
        let source_class = match envelope.source_kind {
            SourceKind::Conversation | SourceKind::Manual => MemorySource::ExplicitUser,
            SourceKind::Discord | SourceKind::Telegram | SourceKind::Slack => {
                MemorySource::ExternalPrimary
            }
            SourceKind::Api | SourceKind::News | SourceKind::Document => {
                MemorySource::ExternalSecondary
            }
        };

        let slot_key = format!(
            "external.{}.{}",
            envelope.source_kind_str(),
            envelope.source_ref
        );
        let source_kind_label = envelope.source_kind_str();
        let semantic_key =
            semantic_dedup_key(&envelope.entity_id, envelope.source_kind, &envelope.content);

        if envelope.signal_tier == SignalTier::Raw
            && self
                .is_source_ref_exact_duplicate(&envelope, &slot_key)
                .await?
        {
            self.observer
                .record_autonomy_lifecycle(AutonomyLifecycleSignal::Deduplicated);
            self.observer
                .record_metric(&ObserverMetric::SignalDedupDropTotal {
                    source_kind: source_kind_label.clone(),
                });
            return Ok(IngestionResult {
                accepted: false,
                slot_key,
                signal_tier: envelope.signal_tier,
                reason: Some("dedup:source_ref_exact".to_string()),
            });
        }

        if envelope.signal_tier == SignalTier::Raw
            && self.is_semantic_duplicate(&envelope, &slot_key).await?
        {
            self.dedup_cache_insert(semantic_key.clone())?;
            self.observer
                .record_autonomy_lifecycle(AutonomyLifecycleSignal::Deduplicated);
            self.observer
                .record_metric(&ObserverMetric::SignalDedupDropTotal {
                    source_kind: source_kind_label.clone(),
                });
            return Ok(IngestionResult {
                accepted: false,
                slot_key,
                signal_tier: envelope.signal_tier,
                reason: Some("dedup:semantic_similar".to_string()),
            });
        }

        if envelope.signal_tier == SignalTier::Raw && self.dedup_cache_contains(&semantic_key)? {
            self.observer
                .record_autonomy_lifecycle(AutonomyLifecycleSignal::Deduplicated);
            self.observer
                .record_metric(&ObserverMetric::SignalDedupDropTotal {
                    source_kind: source_kind_label.clone(),
                });
            return Ok(IngestionResult {
                accepted: false,
                slot_key,
                signal_tier: envelope.signal_tier,
                reason: Some("dedup:semantic_similar".to_string()),
            });
        }

        let source_ref = envelope.source_ref;

        let input = MemoryEventInput::new(
            envelope.entity_id,
            &slot_key,
            MemoryEventType::FactAdded,
            envelope.content,
            source_class,
            envelope.privacy_level,
        )
        .with_signal_tier(envelope.signal_tier)
        .with_source_kind(envelope.source_kind)
        .with_source_ref(&source_ref)
        .with_layer(MemoryLayer::Working)
        .with_importance(0.4)
        .with_provenance(MemoryProvenance::source_reference(
            source_class,
            format!("ingestion:{source_ref}"),
        ));

        enforce_ingestion_write_policy(&input)?;

        self.memory.append_event(input).await?;
        self.dedup_cache_insert(semantic_key)?;
        self.observer
            .record_autonomy_lifecycle(AutonomyLifecycleSignal::Ingested);
        self.observer
            .record_metric(&ObserverMetric::SignalIngestTotal {
                source_kind: source_kind_label,
            });

        Ok(IngestionResult {
            accepted: true,
            slot_key,
            signal_tier: envelope.signal_tier,
            reason: None,
        })
    }
}
