use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A single memory entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub key: String,
    pub content: String,
    pub category: MemoryCategory,
    pub timestamp: String,
    pub session_id: Option<String>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    ExplicitUser,
    ToolVerified,
    System,
    Inferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    Public,
    Private,
    Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForgetMode {
    Soft,
    Hard,
    Tombstone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEventType {
    FactAdded,
    FactUpdated,
    PreferenceSet,
    PreferenceUnset,
    InferredClaim,
    ContradictionMarked,
    SoftDeleted,
    HardDeleted,
    TombstoneWritten,
    SummaryCompacted,
}

impl std::fmt::Display for MemoryEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::FactAdded => "fact_added",
            Self::FactUpdated => "fact_updated",
            Self::PreferenceSet => "preference_set",
            Self::PreferenceUnset => "preference_unset",
            Self::InferredClaim => "inferred_claim",
            Self::ContradictionMarked => "contradiction_marked",
            Self::SoftDeleted => "soft_deleted",
            Self::HardDeleted => "hard_deleted",
            Self::TombstoneWritten => "tombstone_written",
            Self::SummaryCompacted => "summary_compacted",
        };
        write!(f, "{label}")
    }
}

impl std::str::FromStr for MemoryEventType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_lowercase();
        let parsed = match normalized.as_str() {
            "fact_added" => Self::FactAdded,
            "fact_updated" => Self::FactUpdated,
            "preference_set" => Self::PreferenceSet,
            "preference_unset" => Self::PreferenceUnset,
            "inferred_claim" => Self::InferredClaim,
            "contradiction_marked" => Self::ContradictionMarked,
            "soft_deleted" => Self::SoftDeleted,
            "hard_deleted" => Self::HardDeleted,
            "tombstone_written" => Self::TombstoneWritten,
            "summary_compacted" => Self::SummaryCompacted,
            _ => anyhow::bail!("invalid memory event_type: {value}"),
        };
        Ok(parsed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEventInput {
    pub entity_id: String,
    pub slot_key: String,
    pub event_type: MemoryEventType,
    pub value: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub importance: f64,
    pub privacy_level: PrivacyLevel,
    pub occurred_at: String,
}

impl MemoryEventInput {
    pub fn new(
        entity_id: impl Into<String>,
        slot_key: impl Into<String>,
        event_type: MemoryEventType,
        value: impl Into<String>,
        source: MemorySource,
        privacy_level: PrivacyLevel,
    ) -> Self {
        Self {
            entity_id: entity_id.into(),
            slot_key: slot_key.into(),
            event_type,
            value: value.into(),
            source,
            confidence: 0.8,
            importance: 0.5,
            privacy_level,
            occurred_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_importance(mut self, importance: f64) -> Self {
        self.importance = importance.clamp(0.0, 1.0);
        self
    }

    pub fn with_occurred_at(mut self, occurred_at: impl Into<String>) -> Self {
        self.occurred_at = occurred_at.into();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub event_id: String,
    pub entity_id: String,
    pub slot_key: String,
    pub event_type: MemoryEventType,
    pub value: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub importance: f64,
    pub privacy_level: PrivacyLevel,
    pub occurred_at: String,
    pub ingested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallQuery {
    pub entity_id: String,
    pub query: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecallItem {
    pub entity_id: String,
    pub slot_key: String,
    pub value: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub importance: f64,
    pub privacy_level: PrivacyLevel,
    pub score: f64,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSlot {
    pub entity_id: String,
    pub slot_key: String,
    pub value: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub importance: f64,
    pub privacy_level: PrivacyLevel,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgetOutcome {
    pub entity_id: String,
    pub slot_key: String,
    pub mode: ForgetMode,
    pub applied: bool,
}

/// Memory categories for organization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    /// Long-term facts, preferences, decisions
    Core,
    /// Daily session logs
    Daily,
    /// Conversation context
    Conversation,
    /// User-defined custom category
    Custom(String),
}

impl std::fmt::Display for MemoryCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core => write!(f, "core"),
            Self::Daily => write!(f, "daily"),
            Self::Conversation => write!(f, "conversation"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
}

#[async_trait]
pub trait Memory: Send + Sync {
    fn name(&self) -> &str;
    async fn health_check(&self) -> bool;
    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent>;
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
