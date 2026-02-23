mod context;
mod inference;
mod reflect;
mod run;
mod session;
mod types;
mod verify_repair;

// ── Public API re-exports ────────────────────────────────────────
#[allow(unused_imports)]
pub use context::build_context_for_integration;
pub use run::run;
#[allow(unused_imports)]
pub use session::{
    run_main_session_turn_for_integration, run_main_session_turn_for_integration_with_policy,
};
pub use types::IntegrationTurnParams;
pub(super) use types::RuntimeMemoryWriteContext;

// ── Test-only re-exports (visible to tests via super::*) ─────────
#[cfg(test)]
use session::execute_main_session_turn;
#[cfg(test)]
use session::execute_main_session_turn_with_accounting;
#[cfg(test)]
use types::MainSessionTurnParams;
#[cfg(test)]
use types::PERSONA_PER_TURN_CALL_BUDGET;

// ── Test-only crate imports ──────────────────────────────────────
#[cfg(test)]
use crate::config::Config;
#[cfg(test)]
use crate::core::memory::{Memory, MemorySource};
#[cfg(test)]
use crate::core::providers::Provider;
#[cfg(test)]
use crate::runtime::observability::{NoopObserver, Observer};
#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
mod tests;
