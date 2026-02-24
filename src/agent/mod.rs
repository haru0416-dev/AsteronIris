pub mod hooks;
pub mod hooks_leak;
pub mod integration;
pub mod token_estimate;
pub mod tool_loop;

pub use hooks::{HookDecision, PromptHook};
pub use hooks_leak::LeakDetectionHook;
pub use integration::{
    IntegrationRuntimeTurnOptions, IntegrationTurnParams, build_context_for_integration,
    run_main_session_turn_for_integration, run_main_session_turn_for_integration_with_policy,
    run_main_session_turn_for_runtime_with_policy,
};
pub use token_estimate::{estimate_message_tokens, estimate_tokens};
pub use tool_loop::{
    LoopStopReason, ToolCallRecord, ToolLoop, ToolLoopResult, ToolLoopRunParams,
    augment_prompt_with_trust_boundary,
};

/// Compatibility alias: v1 tests import `asteroniris::agent::loop_::*`.
pub mod loop_ {
    pub use super::integration::*;
}
