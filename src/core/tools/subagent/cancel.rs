use crate::core::subagents;
use crate::core::tools::middleware::ExecutionContext;
use crate::core::tools::traits::{Tool, ToolResult};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct SubagentCancelTool;

impl SubagentCancelTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for SubagentCancelTool {
    fn name(&self) -> &str {
        "subagent_cancel"
    }

    fn description(&self) -> &str {
        "Cancel a running sub-agent"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "run_id": { "type": "string", "description": "Sub-agent run id" }
            },
            "required": ["run_id"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let run_id = args
                .get("run_id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Missing 'run_id' parameter"))?;
            subagents::cancel(run_id)?;

            Ok(ToolResult {
                success: true,
                output: serde_json::to_string(&json!({
                    "status": "cancelled",
                    "run_id": run_id,
                }))?,
                error: None,
                attachments: Vec::new(),
            })
        })
    }
}
