use async_trait::async_trait;

pub use super::memory_types::{
    BeliefSlot, CapabilitySupport, ForgetArtifact, ForgetArtifactCheck, ForgetArtifactObservation,
    ForgetArtifactRequirement, ForgetMode, ForgetOutcome, ForgetStatus, MemoryCapabilityMatrix,
    MemoryCategory, MemoryEntry, MemoryEvent, MemoryEventInput, MemoryEventType,
    MemoryInferenceEvent, MemoryLayer, MemoryProvenance, MemoryRecallItem, MemorySource,
    PrivacyLevel, RecallQuery,
};

#[async_trait]
pub trait Memory: Send + Sync {
    fn name(&self) -> &str;
    async fn health_check(&self) -> bool;
    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent>;
    async fn append_inference_event(
        &self,
        event: MemoryInferenceEvent,
    ) -> anyhow::Result<MemoryEvent> {
        self.append_event(event.into_memory_event_input()).await
    }
    async fn append_inference_events(
        &self,
        events: Vec<MemoryInferenceEvent>,
    ) -> anyhow::Result<Vec<MemoryEvent>> {
        let mut persisted = Vec::with_capacity(events.len());
        for event in events {
            persisted.push(self.append_inference_event(event).await?);
        }
        Ok(persisted)
    }
    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>>;
    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>>;
    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome>;
    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize>;
}
