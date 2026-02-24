pub use super::traits::{ExecutionContext, MiddlewareDecision, ToolMiddleware};
use super::types::ToolResult;
use crate::llm::scrub_secret_patterns;
use crate::security::external_content::{ExternalAction, prepare_external_content};
use crate::security::policy::{AutonomyLevel, RateLimitError};
use serde_json::Value;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

const CRITICAL_BOOTSTRAP_FORBIDDEN_WRITE_TARGETS: [&str; 4] =
    ["SOUL.md", "IDENTITY.md", "USER.md", "AGENTS.md"];

fn is_critical_bootstrap_write_target(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|filename| {
            CRITICAL_BOOTSTRAP_FORBIDDEN_WRITE_TARGETS
                .iter()
                .any(|blocked| filename.eq_ignore_ascii_case(blocked))
        })
}

// ── SecurityMiddleware ──────────────────────────────────────────────

#[derive(Debug)]
pub struct SecurityMiddleware;

impl ToolMiddleware for SecurityMiddleware {
    #[allow(clippy::too_many_lines)]
    fn before_execute<'a>(
        &'a self,
        tool_name: &'a str,
        args: &'a Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move {
            if ctx.autonomy_level == AutonomyLevel::ReadOnly {
                match tool_name {
                    "file_read" | "memory_recall" | "browser" => {}
                    _ => {
                        return Ok(MiddlewareDecision::Block(
                            "blocked by security policy: autonomy is read-only".to_string(),
                        ));
                    }
                }
            }

            if let Some(allowed_tools) = &ctx.allowed_tools
                && !allowed_tools.contains(tool_name)
            {
                return Ok(MiddlewareDecision::Block(format!(
                    "blocked by security policy: tool '{tool_name}' is not allowed for this entity"
                )));
            }

            match tool_name {
                "shell" => {
                    let command = args.get("command").and_then(Value::as_str).unwrap_or("");
                    if !ctx.security.is_command_allowed(command) {
                        return Ok(MiddlewareDecision::Block(format!(
                            "blocked by security policy: command not allowed: {command}"
                        )));
                    }
                }
                "file_read" => {
                    let path = args.get("path").and_then(Value::as_str).unwrap_or("");
                    if !ctx.security.is_path_allowed(path) {
                        return Ok(MiddlewareDecision::Block(format!(
                            "blocked by security policy: path not allowed: {path}"
                        )));
                    }

                    let full_path = ctx.workspace_dir.join(path);
                    if let Ok(resolved_path) = tokio::fs::canonicalize(&full_path).await
                        && !ctx.security.is_resolved_path_allowed(&resolved_path)
                    {
                        return Ok(MiddlewareDecision::Block(format!(
                            "blocked by security policy: resolved path escapes workspace: {}",
                            resolved_path.display()
                        )));
                    }
                }
                "file_write" => {
                    let path = args.get("path").and_then(Value::as_str).unwrap_or("");
                    if !ctx.security.is_path_allowed(path) {
                        return Ok(MiddlewareDecision::Block(format!(
                            "blocked by security policy: path not allowed: {path}"
                        )));
                    }
                    if is_critical_bootstrap_write_target(path) {
                        return Ok(MiddlewareDecision::Block(format!(
                            "blocked by security policy: write target is protected bootstrap file: {path}"
                        )));
                    }

                    let full_path = ctx.workspace_dir.join(path);
                    if let Some(parent) = full_path.parent() {
                        let mut candidate: Option<&Path> = Some(parent);
                        while let Some(current) = candidate {
                            if current.exists() {
                                if let Ok(resolved) = tokio::fs::canonicalize(current).await
                                    && !ctx.security.is_resolved_path_allowed(&resolved)
                                {
                                    return Ok(MiddlewareDecision::Block(format!(
                                        "blocked by security policy: resolved path escapes workspace: {}",
                                        resolved.display()
                                    )));
                                }
                                break;
                            }
                            candidate = current.parent();
                        }
                    }
                }
                "memory_governance" => {
                    if !ctx.security.can_act() {
                        return Ok(MiddlewareDecision::Block(
                            "blocked by security policy: autonomy is read-only".to_string(),
                        ));
                    }
                }
                _ => {}
            }

            Ok(MiddlewareDecision::Continue)
        })
    }

    fn after_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {})
    }
}

// ── EntityRateLimitMiddleware ───────────────────────────────────────

#[derive(Debug)]
pub struct EntityRateLimitMiddleware;

impl ToolMiddleware for EntityRateLimitMiddleware {
    fn before_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move {
            match ctx.rate_limiter.check_and_record(&ctx.entity_id) {
                Ok(()) => Ok(MiddlewareDecision::Continue),
                Err(RateLimitError::GlobalExhausted) => Ok(MiddlewareDecision::Block(
                    "blocked by security policy: global action limit exceeded".to_string(),
                )),
                Err(RateLimitError::EntityExhausted { entity_id }) => {
                    Ok(MiddlewareDecision::Block(format!(
                        "blocked by security policy: entity action limit exceeded for '{entity_id}'"
                    )))
                }
            }
        })
    }

    fn after_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {})
    }
}

// ── AuditMiddleware ─────────────────────────────────────────────────

#[derive(Debug)]
pub struct AuditMiddleware;

impl ToolMiddleware for AuditMiddleware {
    fn before_execute<'a>(
        &'a self,
        tool_name: &'a str,
        _args: &'a Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                tool = tool_name,
                entity_id = %ctx.entity_id,
                turn_number = ctx.turn_number,
                "tool execution started"
            );
            Ok(MiddlewareDecision::Continue)
        })
    }

    fn after_execute<'a>(
        &'a self,
        tool_name: &'a str,
        result: &'a mut ToolResult,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(
                tool = tool_name,
                entity_id = %ctx.entity_id,
                turn_number = ctx.turn_number,
                success = result.success,
                has_error = result.error.is_some(),
                "tool execution finished"
            );
        })
    }
}

// ── OutputSizeLimitMiddleware ───────────────────────────────────────

#[derive(Debug)]
pub struct OutputSizeLimitMiddleware;

const MAX_TOOL_OUTPUT_BYTES: usize = 262_144; // 256KB
const MAX_TOOL_OUTPUT_LINES: usize = 4_000;

impl ToolMiddleware for OutputSizeLimitMiddleware {
    fn before_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move { Ok(MiddlewareDecision::Continue) })
    }

    fn after_execute<'a>(
        &'a self,
        tool_name: &'a str,
        result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let original_bytes = result.output.len();
            let original_lines = result.output.lines().count();

            let mut truncated = false;
            let mut output = result.output.clone();

            if original_lines > MAX_TOOL_OUTPUT_LINES {
                let lines: Vec<&str> = output.lines().collect();
                output = lines[..MAX_TOOL_OUTPUT_LINES].join("\n");
                truncated = true;
            }

            if output.len() > MAX_TOOL_OUTPUT_BYTES {
                let mut byte_pos = MAX_TOOL_OUTPUT_BYTES;
                while byte_pos > 0 && !output.is_char_boundary(byte_pos) {
                    byte_pos -= 1;
                }
                output.truncate(byte_pos);
                truncated = true;
            }

            if truncated {
                let metadata_suffix = format!(
                    "\n... [output truncated: {original_bytes} bytes/{original_lines} lines \u{2192} {MAX_TOOL_OUTPUT_BYTES} bytes/{MAX_TOOL_OUTPUT_LINES} lines max]"
                );
                output.push_str(&metadata_suffix);

                tracing::warn!(
                    tool = tool_name,
                    original_bytes,
                    original_lines,
                    max_bytes = MAX_TOOL_OUTPUT_BYTES,
                    max_lines = MAX_TOOL_OUTPUT_LINES,
                    "tool output truncated due to size limits"
                );

                result.output = output;
            }
        })
    }
}

// ── ToolResultSanitizationMiddleware ────────────────────────────────

#[derive(Debug)]
pub struct ToolResultSanitizationMiddleware;

impl ToolMiddleware for ToolResultSanitizationMiddleware {
    fn before_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move { Ok(MiddlewareDecision::Continue) })
    }

    fn after_execute<'a>(
        &'a self,
        tool_name: &'a str,
        result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if !result.output.is_empty() {
                let prepared =
                    prepare_external_content(&format!("tool:{tool_name}:output"), &result.output);
                result.output = prepared.model_input;

                if prepared.action == ExternalAction::Block {
                    result.success = false;
                    result.error =
                        Some("tool output blocked by external-content policy".to_string());
                }
            }

            if let Some(existing_error) = result.error.take() {
                let prepared =
                    prepare_external_content(&format!("tool:{tool_name}:error"), &existing_error);
                if prepared.action == ExternalAction::Block {
                    result.success = false;
                    result.error =
                        Some("tool error blocked by external-content policy".to_string());
                } else {
                    result.error = Some(prepared.model_input);
                }
            }
        })
    }
}

// ── SecretScrubMiddleware ───────────────────────────────────────────

#[derive(Debug)]
pub struct SecretScrubMiddleware;

impl ToolMiddleware for SecretScrubMiddleware {
    fn before_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a Value,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<MiddlewareDecision>> + Send + 'a>> {
        Box::pin(async move { Ok(MiddlewareDecision::Continue) })
    }

    fn after_execute<'a>(
        &'a self,
        _tool_name: &'a str,
        result: &'a mut ToolResult,
        _ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            result.output = scrub_secret_patterns(&result.output).into_owned();
            result.error = result
                .error
                .as_deref()
                .map(scrub_secret_patterns)
                .map(std::borrow::Cow::into_owned);
        })
    }
}

// ── Chain constructor ───────────────────────────────────────────────

/// Returns the default middleware chain used by the tool registry.
pub fn default_middleware_chain() -> Vec<Arc<dyn ToolMiddleware>> {
    vec![
        Arc::new(SecurityMiddleware),
        Arc::new(EntityRateLimitMiddleware),
        Arc::new(AuditMiddleware),
        Arc::new(OutputSizeLimitMiddleware),
        Arc::new(ToolResultSanitizationMiddleware),
        Arc::new(SecretScrubMiddleware),
    ]
}
