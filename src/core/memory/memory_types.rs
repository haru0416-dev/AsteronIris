use crate::security::policy::TenantPolicyContext;
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    ExplicitUser,
    ToolVerified,
    System,
    Inferred,
    ExternalPrimary,
    ExternalSecondary,
}

impl MemorySource {
    #[must_use]
    pub const fn default_confidence(self) -> f64 {
        match self {
            Self::ExplicitUser => 0.95,
            Self::ToolVerified => 0.9,
            Self::System => 0.8,
            Self::Inferred => 0.7,
            Self::ExternalPrimary => 0.75,
            Self::ExternalSecondary => 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryProvenance {
    pub source_class: MemorySource,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_uri: Option<String>,
}

impl MemoryProvenance {
    pub fn source_reference(source_class: MemorySource, reference: impl Into<String>) -> Self {
        Self {
            source_class,
            reference: reference.into(),
            evidence_uri: None,
        }
    }

    pub fn with_evidence_uri(mut self, evidence_uri: impl Into<String>) -> Self {
        self.evidence_uri = Some(evidence_uri.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    Public,
    Private,
    Secret,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForgetMode {
    Soft,
    Hard,
    Tombstone,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    Working,
    Episodic,
    Semantic,
    Procedural,
    Identity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SignalTier {
    Raw,
    Belief,
    Inferred,
    Governance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SourceKind {
    Conversation,
    Discord,
    Telegram,
    Slack,
    Api,
    News,
    Document,
    Manual,
}

const fn default_memory_event_input_layer() -> MemoryLayer {
    MemoryLayer::Working
}

const fn default_inferred_claim_layer() -> MemoryLayer {
    MemoryLayer::Semantic
}

const fn default_contradiction_layer() -> MemoryLayer {
    MemoryLayer::Episodic
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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
    #[serde(default = "default_memory_event_input_layer")]
    pub layer: MemoryLayer,
    pub event_type: MemoryEventType,
    pub value: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub importance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<MemoryProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_tier: Option<SignalTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<SourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
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
            layer: default_memory_event_input_layer(),
            event_type,
            value: value.into(),
            source,
            confidence: source.default_confidence(),
            importance: 0.5,
            provenance: None,
            signal_tier: None,
            source_kind: None,
            source_ref: None,
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

    pub fn with_layer(mut self, layer: MemoryLayer) -> Self {
        self.layer = layer;
        self
    }

    pub fn with_provenance(mut self, provenance: MemoryProvenance) -> Self {
        self.provenance = Some(provenance);
        self
    }

    pub fn with_signal_tier(mut self, signal_tier: SignalTier) -> Self {
        self.signal_tier = Some(signal_tier);
        self
    }

    pub fn with_source_kind(mut self, source_kind: SourceKind) -> Self {
        self.source_kind = Some(source_kind);
        self
    }

    pub fn with_source_ref(mut self, source_ref: impl Into<String>) -> Self {
        self.source_ref = Some(source_ref.into());
        self
    }

    pub fn normalize_for_ingress(mut self) -> anyhow::Result<Self> {
        self.entity_id = normalize_entity_id(&self.entity_id)?;
        self.slot_key = normalize_slot_key(&self.slot_key)?;
        self.confidence = normalize_score(self.confidence, "memory_event_input.confidence")?;
        self.importance = normalize_score(self.importance, "memory_event_input.importance")?;
        if let Some(provenance) = &self.provenance {
            validate_provenance(self.source, provenance)?;
        }
        Ok(self)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<MemoryProvenance>,
    pub privacy_level: PrivacyLevel,
    pub occurred_at: String,
    pub ingested_at: String,
}

fn normalize_score(score: f64, field: &str) -> anyhow::Result<f64> {
    if !score.is_finite() {
        anyhow::bail!("{field} must be finite");
    }
    Ok(score.clamp(0.0, 1.0))
}

fn normalize_entity_id(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_identifier(raw, false);
    if normalized.is_empty() {
        anyhow::bail!("memory_event_input.entity_id must not be empty");
    }
    if normalized.len() > 128 {
        anyhow::bail!("memory_event_input.entity_id must be <= 128 chars");
    }
    Ok(normalized)
}

fn normalize_slot_key(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_identifier(raw, true);
    if normalized.is_empty() {
        anyhow::bail!("memory_event_input.slot_key must not be empty");
    }
    if normalized.len() > 256 {
        anyhow::bail!("memory_event_input.slot_key must be <= 256 chars");
    }
    if !is_valid_slot_key_pattern(&normalized) {
        anyhow::bail!("memory_event_input.slot_key must match taxonomy pattern");
    }
    Ok(normalized)
}

fn is_valid_slot_key_pattern(slot_key: &str) -> bool {
    let mut chars = slot_key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':' | '/'))
}

fn normalize_identifier(raw: &str, allow_slash: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_underscore = false;

    for ch in raw.trim().chars() {
        let allowed = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '_' | '-' | ':')
            || (allow_slash && ch == '/');
        if allowed {
            out.push(ch);
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }

    out.trim_matches('_').to_string()
}

fn validate_provenance(source: MemorySource, provenance: &MemoryProvenance) -> anyhow::Result<()> {
    if provenance.source_class != source {
        anyhow::bail!(
            "memory_event_input.provenance.source_class must match memory_event_input.source"
        );
    }

    if provenance.reference.trim().is_empty() {
        anyhow::bail!("memory_event_input.provenance.reference must not be empty");
    }

    if provenance.reference.len() > 256 {
        anyhow::bail!("memory_event_input.provenance.reference must be <= 256 chars");
    }

    if let Some(uri) = &provenance.evidence_uri
        && uri.trim().is_empty()
    {
        anyhow::bail!("memory_event_input.provenance.evidence_uri must not be empty");
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryInferenceEvent {
    InferredClaim {
        entity_id: String,
        slot_key: String,
        #[serde(default = "default_inferred_claim_layer")]
        layer: MemoryLayer,
        value: String,
        confidence: f64,
        importance: f64,
        privacy_level: PrivacyLevel,
        occurred_at: String,
    },
    ContradictionEvent {
        entity_id: String,
        slot_key: String,
        #[serde(default = "default_contradiction_layer")]
        layer: MemoryLayer,
        value: String,
        confidence: f64,
        importance: f64,
        privacy_level: PrivacyLevel,
        occurred_at: String,
    },
}

impl MemoryInferenceEvent {
    pub fn inferred_claim(
        entity_id: impl Into<String>,
        slot_key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::InferredClaim {
            entity_id: entity_id.into(),
            slot_key: slot_key.into(),
            layer: default_inferred_claim_layer(),
            value: value.into(),
            confidence: 0.7,
            importance: 0.5,
            privacy_level: PrivacyLevel::Private,
            occurred_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn contradiction_marked(
        entity_id: impl Into<String>,
        slot_key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::ContradictionEvent {
            entity_id: entity_id.into(),
            slot_key: slot_key.into(),
            layer: default_contradiction_layer(),
            value: value.into(),
            confidence: 0.85,
            importance: 0.8,
            privacy_level: PrivacyLevel::Private,
            occurred_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_confidence(mut self, confidence: f64) -> Self {
        match &mut self {
            Self::InferredClaim {
                confidence: current,
                ..
            }
            | Self::ContradictionEvent {
                confidence: current,
                ..
            } => {
                *current = confidence.clamp(0.0, 1.0);
            }
        }
        self
    }

    pub fn with_importance(mut self, importance: f64) -> Self {
        match &mut self {
            Self::InferredClaim {
                importance: current,
                ..
            }
            | Self::ContradictionEvent {
                importance: current,
                ..
            } => {
                *current = importance.clamp(0.0, 1.0);
            }
        }
        self
    }

    pub fn with_privacy_level(mut self, privacy_level: PrivacyLevel) -> Self {
        match &mut self {
            Self::InferredClaim {
                privacy_level: current,
                ..
            }
            | Self::ContradictionEvent {
                privacy_level: current,
                ..
            } => {
                *current = privacy_level;
            }
        }
        self
    }

    pub fn with_occurred_at(mut self, occurred_at: impl Into<String>) -> Self {
        let occurred_at = occurred_at.into();
        match &mut self {
            Self::InferredClaim {
                occurred_at: current,
                ..
            }
            | Self::ContradictionEvent {
                occurred_at: current,
                ..
            } => {
                *current = occurred_at;
            }
        }
        self
    }

    pub fn with_layer(mut self, layer: MemoryLayer) -> Self {
        match &mut self {
            Self::InferredClaim { layer: current, .. }
            | Self::ContradictionEvent { layer: current, .. } => {
                *current = layer;
            }
        }
        self
    }

    pub fn into_memory_event_input(self) -> MemoryEventInput {
        match self {
            Self::InferredClaim {
                entity_id,
                slot_key,
                layer,
                value,
                confidence,
                importance,
                privacy_level,
                occurred_at,
            } => MemoryEventInput {
                entity_id,
                slot_key,
                layer,
                event_type: MemoryEventType::InferredClaim,
                value,
                source: MemorySource::Inferred,
                confidence,
                importance,
                provenance: None,
                signal_tier: None,
                source_kind: None,
                source_ref: None,
                privacy_level,
                occurred_at,
            },
            Self::ContradictionEvent {
                entity_id,
                slot_key,
                layer,
                value,
                confidence,
                importance,
                privacy_level,
                occurred_at,
            } => MemoryEventInput {
                entity_id,
                slot_key,
                layer,
                event_type: MemoryEventType::ContradictionMarked,
                value,
                source: MemorySource::System,
                confidence,
                importance,
                provenance: None,
                signal_tier: None,
                source_kind: None,
                source_ref: None,
                privacy_level,
                occurred_at,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallQuery {
    pub entity_id: String,
    pub query: String,
    pub limit: usize,
    #[serde(default)]
    pub policy_context: TenantPolicyContext,
}

impl RecallQuery {
    pub fn new(entity_id: impl Into<String>, query: impl Into<String>, limit: usize) -> Self {
        Self {
            entity_id: entity_id.into(),
            query: query.into(),
            limit,
            policy_context: TenantPolicyContext::default(),
        }
    }

    pub fn with_policy_context(mut self, policy_context: TenantPolicyContext) -> Self {
        self.policy_context = policy_context;
        self
    }

    pub fn enforce_policy(&self) -> anyhow::Result<()> {
        self.policy_context
            .enforce_recall_scope(&self.entity_id)
            .map_err(anyhow::Error::msg)
    }
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
    pub complete: bool,
    pub degraded: bool,
    pub status: ForgetStatus,
    pub artifact_checks: Vec<ForgetArtifactCheck>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForgetStatus {
    Complete,
    Incomplete,
    DegradedNonComplete,
    NotApplied,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForgetArtifact {
    Slot,
    RetrievalUnits,
    RetrievalDocs,
    Caches,
    Ledger,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForgetArtifactRequirement {
    NotGoverned,
    MustExist,
    MustBeAbsent,
    MustBeNonRetrievable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForgetArtifactObservation {
    Absent,
    PresentNonRetrievable,
    PresentRetrievable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForgetArtifactCheck {
    pub artifact: ForgetArtifact,
    pub requirement: ForgetArtifactRequirement,
    pub observed: ForgetArtifactObservation,
    pub satisfied: bool,
}

impl ForgetArtifactCheck {
    #[must_use]
    pub fn new(
        artifact: ForgetArtifact,
        requirement: ForgetArtifactRequirement,
        observed: ForgetArtifactObservation,
    ) -> Self {
        Self {
            artifact,
            requirement,
            observed,
            satisfied: requirement.is_satisfied_by(observed),
        }
    }
}

impl ForgetArtifactRequirement {
    #[must_use]
    pub const fn is_satisfied_by(self, observed: ForgetArtifactObservation) -> bool {
        match self {
            Self::NotGoverned => true,
            Self::MustExist => !matches!(observed, ForgetArtifactObservation::Absent),
            Self::MustBeAbsent => matches!(observed, ForgetArtifactObservation::Absent),
            Self::MustBeNonRetrievable => {
                !matches!(observed, ForgetArtifactObservation::PresentRetrievable)
            }
        }
    }
}

impl ForgetMode {
    #[must_use]
    pub const fn artifact_requirement(self, artifact: ForgetArtifact) -> ForgetArtifactRequirement {
        match (self, artifact) {
            (
                Self::Soft,
                ForgetArtifact::Slot
                | ForgetArtifact::RetrievalUnits
                | ForgetArtifact::RetrievalDocs,
            )
            | (Self::Tombstone, ForgetArtifact::Slot) => {
                ForgetArtifactRequirement::MustBeNonRetrievable
            }
            (Self::Soft, ForgetArtifact::Caches) => ForgetArtifactRequirement::NotGoverned,
            (Self::Soft | Self::Hard | Self::Tombstone, ForgetArtifact::Ledger) => {
                ForgetArtifactRequirement::MustExist
            }
            (
                Self::Hard,
                ForgetArtifact::Slot
                | ForgetArtifact::RetrievalUnits
                | ForgetArtifact::RetrievalDocs
                | ForgetArtifact::Caches,
            )
            | (
                Self::Tombstone,
                ForgetArtifact::RetrievalUnits
                | ForgetArtifact::RetrievalDocs
                | ForgetArtifact::Caches,
            ) => ForgetArtifactRequirement::MustBeAbsent,
        }
    }
}

impl ForgetOutcome {
    #[must_use]
    pub fn from_checks(
        entity_id: impl Into<String>,
        slot_key: impl Into<String>,
        mode: ForgetMode,
        applied: bool,
        degraded: bool,
        artifact_checks: Vec<ForgetArtifactCheck>,
    ) -> Self {
        let complete = applied && artifact_checks.iter().all(|check| check.satisfied);
        let status = if complete {
            ForgetStatus::Complete
        } else if degraded {
            ForgetStatus::DegradedNonComplete
        } else if !applied {
            ForgetStatus::NotApplied
        } else {
            ForgetStatus::Incomplete
        };

        Self {
            entity_id: entity_id.into(),
            slot_key: slot_key.into(),
            mode,
            applied,
            complete,
            degraded,
            status,
            artifact_checks,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Degraded,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryCapabilityMatrix {
    pub backend: &'static str,
    pub forget_soft: CapabilitySupport,
    pub forget_hard: CapabilitySupport,
    pub forget_tombstone: CapabilitySupport,
    pub unsupported_contract: &'static str,
}

impl MemoryCapabilityMatrix {
    pub fn support_for_forget_mode(&self, mode: ForgetMode) -> CapabilitySupport {
        match mode {
            ForgetMode::Soft => self.forget_soft,
            ForgetMode::Hard => self.forget_hard,
            ForgetMode::Tombstone => self.forget_tombstone,
        }
    }

    pub fn require_forget_mode(&self, mode: ForgetMode) -> anyhow::Result<()> {
        if self.support_for_forget_mode(mode) == CapabilitySupport::Unsupported {
            let mode = match mode {
                ForgetMode::Soft => "soft",
                ForgetMode::Hard => "hard",
                ForgetMode::Tombstone => "tombstone",
            };
            anyhow::bail!(
                "memory backend '{}' does not support forget mode '{}'",
                self.backend,
                mode
            );
        }
        Ok(())
    }
}

/// Memory categories for organization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, strum::Display)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum MemoryCategory {
    /// Long-term facts, preferences, decisions
    Core,
    /// Daily session logs
    Daily,
    /// Conversation context
    Conversation,
    /// User-defined custom category
    #[strum(to_string = "{0}")]
    Custom(String),
}

impl MemoryCategory {
    pub fn custom(name: impl Into<String>) -> Self {
        let name = name.into();
        let sanitized: String = name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
            .take(128)
            .collect();
        Self::Custom(sanitized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_category_sanitizes_special_chars() {
        let result = MemoryCategory::custom("hello'; DROP TABLE");
        match result {
            MemoryCategory::Custom(name) => {
                assert_eq!(name, "helloDROPTABLE");
                assert!(!name.contains('\''));
                assert!(!name.contains(';'));
            }
            _ => panic!("Expected Custom variant"),
        }
    }

    #[test]
    fn custom_category_preserves_valid_chars() {
        let result = MemoryCategory::custom("my_custom-category.v1");
        match result {
            MemoryCategory::Custom(name) => {
                assert_eq!(name, "my_custom-category.v1");
            }
            _ => panic!("Expected Custom variant"),
        }
    }

    #[test]
    fn custom_category_caps_length() {
        let long_name = "a".repeat(200);
        let result = MemoryCategory::custom(&long_name);
        match result {
            MemoryCategory::Custom(name) => {
                assert!(
                    name.len() <= 128,
                    "Name should be capped at 128 chars, got {}",
                    name.len()
                );
            }
            _ => panic!("Expected Custom variant"),
        }
    }

    #[test]
    fn normalize_for_ingress_sanitizes_entity_and_slot() {
        let input = MemoryEventInput::new(
            " person:User / A ",
            "external.channel.discord.user/1?x",
            MemoryEventType::FactAdded,
            "v",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );

        let normalized = input.normalize_for_ingress().unwrap();
        assert_eq!(normalized.entity_id, "person:User_A");
        assert_eq!(normalized.slot_key, "external.channel.discord.user/1_x");
    }

    #[test]
    fn normalize_for_ingress_rejects_empty_identifiers() {
        let input = MemoryEventInput::new(
            "   ",
            "slot",
            MemoryEventType::FactAdded,
            "v",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );
        let err = input.normalize_for_ingress().unwrap_err().to_string();
        assert!(err.contains("entity_id"));

        let input = MemoryEventInput::new(
            "entity",
            "   ",
            MemoryEventType::FactAdded,
            "v",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );
        let err = input.normalize_for_ingress().unwrap_err().to_string();
        assert!(err.contains("slot_key"));
    }

    #[test]
    fn normalize_for_ingress_rejects_invalid_slot_key_pattern() {
        let input = MemoryEventInput::new(
            "entity",
            ".invalid-slot",
            MemoryEventType::FactAdded,
            "v",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        );
        let err = input.normalize_for_ingress().unwrap_err().to_string();
        assert!(err.contains("slot_key must match taxonomy pattern"));
    }
}
