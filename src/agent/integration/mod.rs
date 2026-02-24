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
#[allow(unused_imports)]
pub use session::{
    run_main_session_turn_for_integration, run_main_session_turn_for_integration_with_policy,
    run_main_session_turn_for_runtime_with_policy,
};
pub(super) use types::RuntimeMemoryWriteContext;
pub use types::{IntegrationRuntimeTurnOptions, IntegrationTurnParams};
