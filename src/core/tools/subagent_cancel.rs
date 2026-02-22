use super::traits::{Tool, ToolResult};
use crate::core::subagents;
use crate::core::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use serde_json::{Value, json};

pub struct SubagentCancelTool;

impl SubagentCancelTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
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

    async fn execute(&self, args: Value, _ctx: &ExecutionContext) -> anyhow::Result<ToolResult> {
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
    }
}
