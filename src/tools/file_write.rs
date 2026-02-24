use super::common::{failed_tool_result, workspace_path_property};
use super::traits::{ExecutionContext, Tool};
use super::types::ToolResult;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;

/// Write file contents with path sandboxing.
pub struct FileWriteTool;

impl FileWriteTool {
    pub const fn new() -> Self {
        Self
    }
}

impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write contents to a file in the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": workspace_path_property(),
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

            if !ctx.security.is_path_allowed(path) {
                return Ok(failed_tool_result(
                    "blocked by security policy: path not allowed",
                ));
            }

            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' parameter"))?;

            let full_path = ctx.workspace_dir.join(path);

            let Some(parent) = full_path.parent() else {
                return Ok(failed_tool_result("Invalid path: missing parent directory"));
            };

            match tokio::fs::metadata(parent).await {
                Ok(meta) => {
                    if !meta.is_dir() {
                        return Ok(failed_tool_result(format!(
                            "Invalid path: parent is not a directory: {}",
                            parent.display()
                        )));
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    tokio::fs::create_dir_all(parent).await?;
                }
                Err(e) => {
                    return Ok(failed_tool_result(format!(
                        "Failed to inspect parent directory: {e}"
                    )));
                }
            }

            // Resolve parent AFTER creation to block symlink escapes.
            let resolved_parent = match tokio::fs::canonicalize(parent).await {
                Ok(p) => p,
                Err(e) => {
                    return Ok(failed_tool_result(format!(
                        "Failed to resolve file path: {e}"
                    )));
                }
            };

            let Some(file_name) = full_path.file_name() else {
                return Ok(failed_tool_result("Invalid path: missing file name"));
            };

            let resolved_target = resolved_parent.join(file_name);

            if !ctx.security.is_resolved_path_allowed(&resolved_parent) {
                return Ok(failed_tool_result(
                    "blocked by security policy: resolved path escapes workspace",
                ));
            }

            // If the target already exists and is a symlink, refuse to follow it
            if let Ok(meta) = tokio::fs::symlink_metadata(&resolved_target).await
                && meta.file_type().is_symlink()
            {
                return Ok(failed_tool_result(format!(
                    "Refusing to write through symlink: {}",
                    resolved_target.display()
                )));
            }

            match tokio::fs::write(&resolved_target, content).await {
                Ok(()) => Ok(ToolResult {
                    success: true,
                    output: format!("Written {} bytes to {path}", content.len()),
                    error: None,
                    attachments: Vec::new(),
                }),
                Err(e) => Ok(failed_tool_result(format!("Failed to write file: {e}"))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::common::test_security_policy;
    use crate::tools::traits::ExecutionContext;

    #[test]
    fn file_write_name() {
        let tool = FileWriteTool::new();
        assert_eq!(tool.name(), "file_write");
    }

    #[test]
    fn file_write_schema_has_path_and_content() {
        let tool = FileWriteTool::new();
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["content"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("path")));
        assert!(required.contains(&json!("content")));
    }

    #[tokio::test]
    async fn file_write_creates_file() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_write");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(dir.clone()));
        let result = tool
            .execute(json!({"path": "out.txt", "content": "written!"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("8 bytes"));

        let content = tokio::fs::read_to_string(dir.join("out.txt"))
            .await
            .unwrap();
        assert_eq!(content, "written!");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_write_creates_parent_dirs() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_write_nested");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(dir.clone()));
        let result = tool
            .execute(json!({"path": "a/b/c/deep.txt", "content": "deep"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);

        let content = tokio::fs::read_to_string(dir.join("a/b/c/deep.txt"))
            .await
            .unwrap();
        assert_eq!(content, "deep");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_write_overwrites_existing() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_write_overwrite");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("exist.txt"), "old")
            .await
            .unwrap();

        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(dir.clone()));
        let result = tool
            .execute(json!({"path": "exist.txt", "content": "new"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);

        let content = tokio::fs::read_to_string(dir.join("exist.txt"))
            .await
            .unwrap();
        assert_eq!(content, "new");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_write_missing_path_param() {
        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(std::env::temp_dir()));
        let result = tool.execute(json!({"content": "data"}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_write_missing_content_param() {
        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(std::env::temp_dir()));
        let result = tool.execute(json!({"path": "file.txt"}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_write_rejects_path_traversal() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_write_traversal");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(dir.clone()));
        let result = tool
            .execute(
                json!({"path": "../../etc/evil.txt", "content": "pwned"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(
            result
                .error
                .as_ref()
                .unwrap()
                .contains("blocked by security policy")
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_write_rejects_symlink_escape() {
        let workspace = std::env::temp_dir().join("asteroniris_test_file_write_symlink");
        let outside = std::env::temp_dir().join("asteroniris_test_file_write_symlink_outside");
        let _ = tokio::fs::remove_dir_all(&workspace).await;
        let _ = tokio::fs::remove_dir_all(&outside).await;
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();

        #[cfg(unix)]
        {
            tokio::fs::symlink(&outside, workspace.join("escape_dir"))
                .await
                .unwrap();

            let tool = FileWriteTool::new();
            let ctx = ExecutionContext::test_default(test_security_policy(workspace.clone()));
            let result = tool
                .execute(
                    json!({"path": "escape_dir/evil.txt", "content": "pwned"}),
                    &ctx,
                )
                .await
                .unwrap();
            assert!(!result.success);
            assert!(
                result
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("blocked by security policy")
            );
        }

        let _ = tokio::fs::remove_dir_all(&workspace).await;
        let _ = tokio::fs::remove_dir_all(&outside).await;
    }

    #[tokio::test]
    async fn file_write_empty_content() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_write_empty");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileWriteTool::new();
        let ctx = ExecutionContext::test_default(test_security_policy(dir.clone()));
        let result = tool
            .execute(json!({"path": "empty.txt", "content": ""}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.contains("0 bytes"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
