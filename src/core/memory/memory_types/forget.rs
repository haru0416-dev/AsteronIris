use serde::{Deserialize, Serialize};

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
pub enum ForgetMode {
    Soft,
    Hard,
    Tombstone,
}
