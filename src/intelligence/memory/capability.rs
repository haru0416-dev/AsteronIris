use super::traits::{CapabilitySupport, ForgetMode, Memory, MemoryCapabilityMatrix};

const SQLITE_CAPABILITY_MATRIX: MemoryCapabilityMatrix = MemoryCapabilityMatrix {
    backend: "sqlite",
    forget_soft: CapabilitySupport::Supported,
    forget_hard: CapabilitySupport::Supported,
    forget_tombstone: CapabilitySupport::Supported,
    unsupported_contract: "sqlite supports soft/hard/tombstone forget semantics",
};

#[cfg(feature = "vector-search")]
const LANCEDB_CAPABILITY_MATRIX: MemoryCapabilityMatrix = MemoryCapabilityMatrix {
    backend: "lancedb",
    forget_soft: CapabilitySupport::Degraded,
    forget_hard: CapabilitySupport::Supported,
    forget_tombstone: CapabilitySupport::Degraded,
    unsupported_contract: "lancedb soft/tombstone are marker rewrites; hard forget removes projection",
};

const MARKDOWN_CAPABILITY_MATRIX: MemoryCapabilityMatrix = MemoryCapabilityMatrix {
    backend: "markdown",
    forget_soft: CapabilitySupport::Degraded,
    forget_hard: CapabilitySupport::Unsupported,
    forget_tombstone: CapabilitySupport::Degraded,
    unsupported_contract: "markdown is append-only; hard forget cannot physically delete",
};

#[cfg(feature = "vector-search")]
const BACKEND_CAPABILITY_MATRIX: [MemoryCapabilityMatrix; 3] = [
    SQLITE_CAPABILITY_MATRIX,
    LANCEDB_CAPABILITY_MATRIX,
    MARKDOWN_CAPABILITY_MATRIX,
];

#[cfg(not(feature = "vector-search"))]
const BACKEND_CAPABILITY_MATRIX: [MemoryCapabilityMatrix; 2] =
    [SQLITE_CAPABILITY_MATRIX, MARKDOWN_CAPABILITY_MATRIX];

pub fn backend_capability_matrix() -> &'static [MemoryCapabilityMatrix] {
    &BACKEND_CAPABILITY_MATRIX
}

pub fn capability_matrix_for_backend(backend: &str) -> Option<MemoryCapabilityMatrix> {
    let normalized = if backend == "none" {
        "markdown"
    } else {
        backend
    };
    BACKEND_CAPABILITY_MATRIX
        .iter()
        .find(|capability| capability.backend == normalized)
        .copied()
}

#[must_use]
pub fn capability_matrix_for_memory(memory: &dyn Memory) -> MemoryCapabilityMatrix {
    capability_matrix_for_backend(memory.name()).unwrap_or(MARKDOWN_CAPABILITY_MATRIX)
}

pub fn ensure_forget_mode_supported(memory: &dyn Memory, mode: ForgetMode) -> anyhow::Result<()> {
    capability_matrix_for_memory(memory).require_forget_mode(mode)
}
