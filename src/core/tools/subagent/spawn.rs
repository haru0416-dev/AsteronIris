use crate::core::subagents;
use crate::core::tools::middleware::ExecutionContext;
use crate::core::tools::traits::{Tool, ToolResult};
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct SubagentSpawnTool;

impl SubagentSpawnTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for SubagentSpawnTool {
    fn name(&self) -> &str {
        "subagent_spawn"
    }

    fn description(&self) -> &str {
        "Spawn an isolated sub-agent run or execute inline task"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": { "type": "string", "description": "Task instruction for sub-agent" },
                "label": { "type": "string", "description": "Optional run label" },
                "model": { "type": "string", "description": "Optional model override" },
                "run_in_background": { "type": "boolean", "description": "If false, run inline and return output", "default": true }
            },
            "required": ["task"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let task = args
                .get("task")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter"))?
                .trim()
                .to_string();
            if task.is_empty() {
                anyhow::bail!("'task' must not be empty");
            }
            let label = args
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let model = args
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let background = args
                .get("run_in_background")
                .and_then(Value::as_bool)
                .unwrap_or(true);

            if background {
                let snapshot = subagents::spawn(task, label, model)?;
                return Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string(&json!({
                        "status": "accepted",
                        "run_id": snapshot.run_id,
                        "started_at": snapshot.started_at,
                    }))?,
                    error: None,
                    attachments: Vec::new(),
                });
            }

            let output = subagents::run_inline(task, model).await?;
            Ok(ToolResult {
                success: true,
                output: serde_json::to_string(&json!({
                    "status": "completed",
                    "output": output,
                }))?,
                error: None,
                attachments: Vec::new(),
            })
        })
    }
}
