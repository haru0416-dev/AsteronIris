use super::traits::{Tool, ToolResult};
use crate::core::subagents;
use crate::core::tools::middleware::ExecutionContext;
use serde_json::{Value, json};
use std::future::Future;
use std::pin::Pin;

pub struct DelegateTool;

impl DelegateTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Unified task delegation tool: run, status, list, cancel"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["run", "status", "list", "cancel"],
                    "description": "Delegate operation"
                },
                "task": { "type": "string", "description": "Task instruction (required for action=run)" },
                "run_id": { "type": "string", "description": "Run id (required for action=status|cancel)" },
                "label": { "type": "string", "description": "Optional run label for action=run" },
                "model": { "type": "string", "description": "Optional model override for action=run" },
                "run_in_background": { "type": "boolean", "description": "For action=run: true=async, false=sync", "default": false }
            },
            "required": ["action"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' parameter"))?;

            match action {
                "run" => {
                    let task = args
                        .get("task")
                        .and_then(Value::as_str)
                        .ok_or_else(|| anyhow::anyhow!("Missing 'task' parameter for action=run"))?
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
                    let run_in_background = args
                        .get("run_in_background")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);

                    if run_in_background {
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
                }
                "status" => {
                    let run_id = args.get("run_id").and_then(Value::as_str).ok_or_else(|| {
                        anyhow::anyhow!("Missing 'run_id' parameter for action=status")
                    })?;
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
                }
                "list" => Ok(ToolResult {
                    success: true,
                    output: serde_json::to_string(&subagents::list())?,
                    error: None,
                    attachments: Vec::new(),
                }),
                "cancel" => {
                    let run_id = args.get("run_id").and_then(Value::as_str).ok_or_else(|| {
                        anyhow::anyhow!("Missing 'run_id' parameter for action=cancel")
                    })?;
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
                other => anyhow::bail!("unsupported action: {other}"),
            }
        })
    }
}
