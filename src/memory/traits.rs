use super::associations::MemoryAssociation;
use std::future::Future;
use std::pin::Pin;

pub use super::types::{
    BeliefSlot, CapabilitySupport, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, ForgetStatus, MemoryCapabilityMatrix,
    MemoryCategory, MemoryEntry, MemoryEvent, MemoryEventInput, MemoryEventType,
    MemoryInferenceEvent, MemoryLayer, MemoryProvenance, MemoryRecallItem, MemorySource,
    PrivacyLevel, RecallQuery, SignalTier, SourceKind,
};

pub trait Memory: Send + Sync {
    fn name(&self) -> &str;
    fn health_check(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
    fn append_event(
        &self,
        input: MemoryEventInput,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MemoryEvent>> + Send + '_>>;
    fn append_inference_event(
        &self,
        event: MemoryInferenceEvent,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MemoryEvent>> + Send + '_>> {
        Box::pin(async move { self.append_event(event.into_memory_event_input()).await })
    }
    fn append_inference_events(
        &self,
        events: Vec<MemoryInferenceEvent>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryEvent>>> + Send + '_>> {
        Box::pin(async move {
            let mut persisted = Vec::with_capacity(events.len());
            for event in events {
                persisted.push(self.append_inference_event(event).await?);
            }
            Ok(persisted)
        })
    }
    fn recall_scoped(
        &self,
        query: RecallQuery,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryRecallItem>>> + Send + '_>>;
    fn recall_phased(
        &self,
        query: RecallQuery,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryRecallItem>>> + Send + '_>> {
        Box::pin(async move { self.recall_scoped(query).await })
    }
    fn resolve_slot<'a>(
        &'a self,
        entity_id: &'a str,
        slot_key: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Option<BeliefSlot>>> + Send + 'a>>;
    fn forget_slot<'a>(
        &'a self,
        entity_id: &'a str,
        slot_key: &'a str,
        mode: ForgetMode,
        reason: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ForgetOutcome>> + Send + 'a>>;
    fn count_events<'a>(
        &'a self,
        entity_id: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<usize>> + Send + 'a>>;

    // ── NEW: association graph ──────────────────────────────────

    /// Store an association between two memory entries.
    fn add_association<'a>(
        &'a self,
        _association: MemoryAssociation,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    /// Retrieve associations for a given memory entry.
    fn get_associations<'a>(
        &'a self,
        _entry_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<MemoryAssociation>>> + Send + 'a>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}
