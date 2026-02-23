use super::coordination::{AggregatedResult, CoordinationSession};
use super::roles::AgentRole;
use anyhow::{Result, bail};

/// Dispatch tasks to multiple agents in parallel.
/// Full implementation in T11 (Wave 3).
#[allow(clippy::unused_async)]
pub async fn dispatch_parallel(
    _session: &CoordinationSession,
    _tasks: Vec<(AgentRole, String)>,
) -> Result<AggregatedResult> {
    bail!("dispatch_parallel: not yet implemented (Wave 3 T11)")
}
