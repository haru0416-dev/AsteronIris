use super::deps::AgentDeps;
use super::events::{EventSender, ProcessEvent};
use crate::agent::tool_loop::{ToolLoop, ToolLoopResult, ToolLoopRunParams};
use crate::llm::streaming::StreamSink;
use crate::llm::types::{ContentBlock, ProviderMessage};
use crate::security::policy::{AutonomyLevel, EntityRateLimiter, TenantPolicyContext};
use crate::tools::ExecutionContext;
use std::sync::Arc;

/// Input parameters for a single worker invocation.
pub struct WorkerParams {
    pub entity_id: String,
    pub system_prompt: String,
    pub user_message: String,
    pub image_content: Vec<ContentBlock>,
    pub model: String,
    pub temperature: f64,
    pub max_tool_iterations: u32,
    pub conversation_history: Vec<ProviderMessage>,
    pub stream_sink: Option<Arc<dyn StreamSink>>,
}

/// Output from a single worker invocation.
pub struct WorkerResult {
    pub tool_loop_result: ToolLoopResult,
    pub entity_id: String,
}

/// Execute a single-turn worker: build context, run the tool loop, emit events.
pub async fn run_worker(
    deps: &AgentDeps,
    params: WorkerParams,
    events: &EventSender,
) -> anyhow::Result<WorkerResult> {
    #[allow(clippy::cast_possible_truncation)]
    let turn = params.conversation_history.len() as u32 + 1;

    let _ = events.send(ProcessEvent::WorkerStarted {
        entity_id: params.entity_id.clone(),
        turn,
    });

    let provider = deps.llm.get_provider()?;

    let ctx = ExecutionContext {
        workspace_dir: deps.security.workspace_dir.clone(),
        security: Arc::clone(&deps.security),
        autonomy_level: AutonomyLevel::Supervised,
        entity_id: params.entity_id.clone(),
        turn_number: turn,
        allowed_tools: None,
        rate_limiter: Arc::new(EntityRateLimiter::new(100, 20)),
        tenant_context: TenantPolicyContext::disabled(),
    };

    let tool_loop = ToolLoop::new(Arc::clone(&deps.tool_registry), params.max_tool_iterations);

    let run_params = ToolLoopRunParams {
        provider: provider.as_ref(),
        system_prompt: &params.system_prompt,
        user_message: &params.user_message,
        image_content: &params.image_content,
        model: &params.model,
        temperature: params.temperature,
        ctx: &ctx,
        stream_sink: params.stream_sink,
        conversation_history: &params.conversation_history,
        hooks: &deps.hooks,
    };

    let tool_loop_result = tool_loop.run(run_params).await?;

    let _ = events.send(ProcessEvent::WorkerCompleted {
        entity_id: params.entity_id.clone(),
        turn,
        tokens_used: tool_loop_result.tokens_used,
    });

    Ok(WorkerResult {
        tool_loop_result,
        entity_id: params.entity_id,
    })
}
