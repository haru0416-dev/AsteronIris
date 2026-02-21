use super::traits::{Tool, ToolResult};
use crate::core::tools::middleware::ExecutionContext;
use async_trait::async_trait;
use serde_json::json;
use tokio::io::AsyncReadExt;

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Read file contents with path sandboxing
pub struct FileReadTool;

impl FileReadTool {
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file in the workspace"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Relative path to the file within the workspace"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' parameter"))?;

        let full_path = ctx.workspace_dir.join(path);

        // Resolve path before reading to block symlink escapes.
        let resolved_path = match tokio::fs::canonicalize(&full_path).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to resolve file path: {e}")),

                    attachments: Vec::new(),
                });
            }
        };

        let mut file = match tokio::fs::File::open(&resolved_path).await {
            Ok(file) => file,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file: {e}")),

                    attachments: Vec::new(),
                });
            }
        };

        // Check file size AFTER canonicalization to prevent TOCTOU symlink bypass
        let metadata = match file.metadata().await {
            Ok(metadata) => metadata,
            Err(e) => {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to read file metadata: {e}")),

                    attachments: Vec::new(),
                });
            }
        };
        if metadata.len() > MAX_FILE_SIZE {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!(
                    "File too large: {} bytes (limit: {MAX_FILE_SIZE} bytes)",
                    metadata.len()
                )),

                attachments: Vec::new(),
            });
        }

        #[allow(clippy::cast_possible_truncation)]
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        if let Err(e) = file.read_to_end(&mut bytes).await {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(format!("Failed to read file: {e}")),

                attachments: Vec::new(),
            });
        }

        match String::from_utf8(bytes) {
            Ok(contents) => Ok(ToolResult {
                success: true,
                output: contents,
                error: None,

                attachments: Vec::new(),
            }),
            Err(_) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some("Failed to read file: file is not valid UTF-8".to_string()),

                attachments: Vec::new(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tools::middleware::ExecutionContext;
    use crate::security::{AutonomyLevel, SecurityPolicy};
    use std::sync::Arc;

    fn test_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: workspace,
            ..SecurityPolicy::default()
        })
    }

    #[test]
    fn file_read_name() {
        let tool = FileReadTool::new();
        assert_eq!(tool.name(), "file_read");
    }

    #[test]
    fn file_read_schema_has_path() {
        let tool = FileReadTool::new();
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["path"].is_object());
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("path"))
        );
    }

    #[tokio::test]
    async fn file_read_existing_file() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_read");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("test.txt"), "hello world")
            .await
            .unwrap();

        let tool = FileReadTool::new();
        let ctx = ExecutionContext::test_default(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "test.txt"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "hello world");
        assert!(result.error.is_none());

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_read_nonexistent_file() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_read_missing");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let tool = FileReadTool::new();
        let ctx = ExecutionContext::test_default(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "nope.txt"}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("Failed to resolve"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_read_missing_path_param() {
        let tool = FileReadTool::new();
        let ctx = ExecutionContext::test_default(test_security(std::env::temp_dir()));
        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_read_empty_file() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_read_empty");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("empty.txt"), "").await.unwrap();

        let tool = FileReadTool::new();
        let ctx = ExecutionContext::test_default(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "empty.txt"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_read_nested_path() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_read_nested");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(dir.join("sub/dir"))
            .await
            .unwrap();
        tokio::fs::write(dir.join("sub/dir/deep.txt"), "deep content")
            .await
            .unwrap();

        let tool = FileReadTool::new();
        let ctx = ExecutionContext::test_default(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "sub/dir/deep.txt"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "deep content");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn file_read_rejects_oversized_file() {
        let dir = std::env::temp_dir().join("asteroniris_test_file_read_large");
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::create_dir_all(&dir).await.unwrap();

        // Create a file just over 10 MB
        let big = vec![b'x'; 10 * 1024 * 1024 + 1];
        tokio::fs::write(dir.join("huge.bin"), &big).await.unwrap();

        let tool = FileReadTool::new();
        let ctx = ExecutionContext::test_default(test_security(dir.clone()));
        let result = tool
            .execute(json!({"path": "huge.bin"}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("File too large"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
