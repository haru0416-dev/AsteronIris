use crate::core::subagents;
use crate::core::tools::middleware::ExecutionContext;
use crate::core::tools::traits::{Tool, ToolResult};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct SubagentOutputTool;

impl SubagentOutputTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for SubagentOutputTool {
    fn name(&self) -> &str {
        "subagent_output"
    }

    fn description(&self) -> &str {
        "Get status/output for a spawned sub-agent run"
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
            let Some(snapshot) = subagents::get(run_id) else {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("subagent run not found: {run_id}")),
                    attachments: Vec::new(),
                });
            };

            Ok(ToolResult {
                success: true,
                output: serde_json::to_string(&snapshot)?,
                error: None,
                attachments: Vec::new(),
            })
        })
    }
}
