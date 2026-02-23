//! Security pattern detection for the `SkillForge` gate.
//!
//! Four layers of analysis:
//! - Layer 1: Code patterns (credential harvesting, obfuscation, exec)
//! - Layer 2: Metadata patterns (bad names, typosquatting, binary artifacts)
//! - Layer 3: Markdown/doc injection (instruction override, priv escalation)
//! - Layer 4: Provenance (mutable refs, hash mismatch)

use serde::{Deserialize, Serialize};

mod code;
mod markdown;
mod metadata;
mod provenance;
mod shared;

pub use code::{CodeSignals, code_reasons, detect_code_signals};
pub use markdown::detect_markdown_reasons;
pub use metadata::detect_metadata_reasons;
pub use provenance::detect_provenance_reasons;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    // Layer 1 — Code (reject combinations)
    CredentialHarvest,
    EncodedPayload,
    BuildScriptAbuse,
    NativeLoading,
    ObfuscationExec,

    // Layer 1 — Code (quarantine singles)
    Subprocess,
    UndeclaredNetwork,
    EnvRead,
    UndeclaredFilesystem,
    UnsafeBlock,
    Deserialization,
    HighEntropy,

    // Layer 2 — Metadata (reject)
    BadPatternName,
    Typosquatting,
    BinaryArtifact,

    // Layer 2 — Metadata (quarantine)
    NewAuthor,
    UnstableOwnership,
    Unmaintained,
    NoLicense,

    // Layer 3 — Markdown/doc injection (reject)
    InstructionOverride,
    PrivilegeEscalation,
    SecretExfiltration,
    SecurityDisable,
    ConfigTampering,

    // Layer 3 — Markdown/doc injection (quarantine)
    CapabilityMismatch,
    PermissionRequest,
    ToolJailbreak,

    // Layer 4 — Provenance (reject)
    MutableRef,
    HashMismatch,

    // Layer 4 — Provenance (quarantine)
    MissingProvenance,
}

impl ReasonCode {
    pub fn is_reject(&self) -> bool {
        matches!(
            self,
            Self::CredentialHarvest
                | Self::EncodedPayload
                | Self::BuildScriptAbuse
                | Self::NativeLoading
                | Self::ObfuscationExec
                | Self::BadPatternName
                | Self::Typosquatting
                | Self::BinaryArtifact
                | Self::InstructionOverride
                | Self::PrivilegeEscalation
                | Self::SecretExfiltration
                | Self::SecurityDisable
                | Self::ConfigTampering
                | Self::MutableRef
                | Self::HashMismatch
        )
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::CredentialHarvest => {
                "code reads credentials and accesses network (exfiltration pattern)"
            }
            Self::EncodedPayload => "code decodes data and executes it (encoded payload pattern)",
            Self::BuildScriptAbuse => "build.rs accesses network (supply-chain attack vector)",
            Self::NativeLoading => "code loads native libraries (dlopen/libloading)",
            Self::ObfuscationExec => "code combines obfuscation with execution",
            Self::Subprocess => "code spawns subprocesses",
            Self::UndeclaredNetwork => "code accesses network without declaring net capability",
            Self::EnvRead => "code reads environment variables",
            Self::UndeclaredFilesystem => {
                "code accesses filesystem without declaring read/write capability"
            }
            Self::UnsafeBlock => "code contains unsafe blocks",
            Self::Deserialization => "code performs deserialization of untrusted data",
            Self::HighEntropy => "code contains high-entropy strings (potential obfuscation)",
            Self::BadPatternName => "name matches known malicious patterns",
            Self::Typosquatting => "name is suspiciously similar to a popular package",
            Self::BinaryArtifact => "repository contains binary artifacts (.so/.dll/.dylib/.wasm)",
            Self::NewAuthor => "author has no established history",
            Self::UnstableOwnership => "repository ownership changed recently",
            Self::Unmaintained => "repository not updated in 90+ days",
            Self::NoLicense => "repository has no license file",
            Self::InstructionOverride => "documentation attempts to override system instructions",
            Self::PrivilegeEscalation => "documentation attempts privilege escalation",
            Self::SecretExfiltration => "documentation attempts secret exfiltration",
            Self::SecurityDisable => "documentation attempts to disable security features",
            Self::ConfigTampering => "documentation attempts to tamper with configuration",
            Self::CapabilityMismatch => "declared capabilities don't match detected code patterns",
            Self::PermissionRequest => "documentation requests elevated permissions",
            Self::ToolJailbreak => "documentation attempts tool policy bypass",
            Self::MutableRef => "source reference is a mutable branch, not a pinned commit SHA",
            Self::HashMismatch => "content hash does not match stored provenance",
            Self::MissingProvenance => "no provenance data (commit SHA or content hash) available",
        }
    }
}

impl std::fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.description())
    }
}
