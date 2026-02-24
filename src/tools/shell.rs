use super::traits::{ExecutionContext, Tool};
use super::types::ToolResult;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// Maximum shell command execution time before kill.
const SHELL_TIMEOUT_SECS: u64 = 60;
/// Maximum output size in bytes (1 MB).
const MAX_OUTPUT_BYTES: usize = 1_048_576;
/// Environment variables safe to pass to shell commands.
/// Only functional variables are included -- never API keys or secrets.
const SAFE_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "TERM", "LANG", "LC_ALL", "LC_CTYPE", "USER", "SHELL",
];

/// Shell command execution tool with sandboxing.
pub struct ShellTool;

impl ShellTool {
    pub const fn new() -> Self {
        Self
    }
}

impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command in the workspace directory"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                }
            },
            "required": ["command"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        ctx: &'a ExecutionContext,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'command' parameter"))?;

            if !ctx.security.is_command_allowed(command) {
                return Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("blocked by security policy: command not allowed".to_string()),
                    attachments: Vec::new(),
                });
            }

            // Execute with timeout to prevent hanging commands.
            // Clear the environment to prevent leaking API keys and other secrets
            // (CWE-200), then re-add only safe, functional variables.
            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c")
                .arg(command)
                .current_dir(&ctx.workspace_dir)
                .env_clear();

            for var in SAFE_ENV_VARS {
                if let Ok(val) = std::env::var(var) {
                    cmd.env(var, val);
                }
            }

            // Override TMPDIR to a controlled workspace-local directory
            let controlled_tmp = ctx.workspace_dir.join(".asteroniris-tmp");
            if !controlled_tmp.exists() {
                std::fs::create_dir_all(&controlled_tmp)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        &controlled_tmp,
                        std::fs::Permissions::from_mode(0o700),
                    )?;
                }
            }
            cmd.env("TMPDIR", &controlled_tmp);

            let result =
                tokio::time::timeout(Duration::from_secs(SHELL_TIMEOUT_SECS), cmd.output()).await;

            match result {
                Ok(Ok(output)) => {
                    let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();

                    // Truncate output to prevent OOM
                    if stdout.len() > MAX_OUTPUT_BYTES {
                        stdout.truncate(stdout.floor_char_boundary(MAX_OUTPUT_BYTES));
                        stdout.push_str("\n... [output truncated at 1MB]");
                    }
                    if stderr.len() > MAX_OUTPUT_BYTES {
                        stderr.truncate(stderr.floor_char_boundary(MAX_OUTPUT_BYTES));
                        stderr.push_str("\n... [stderr truncated at 1MB]");
                    }

                    Ok(ToolResult {
                        success: output.status.success(),
                        output: stdout,
                        error: if stderr.is_empty() {
                            None
                        } else {
                            Some(stderr)
                        },
                        attachments: Vec::new(),
                    })
                }
                Ok(Err(e)) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!("Failed to execute command: {e}")),
                    attachments: Vec::new(),
                }),
                Err(_) => Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some(format!(
                        "Command timed out after {SHELL_TIMEOUT_SECS}s and was killed"
                    )),
                    attachments: Vec::new(),
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{AutonomyLevel, SecurityPolicy};
    use crate::tools::traits::ExecutionContext;
    use std::sync::Arc;

    fn test_security(autonomy: AutonomyLevel) -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy,
            workspace_dir: std::env::temp_dir(),
            ..SecurityPolicy::default()
        })
    }

    #[test]
    fn shell_tool_name() {
        let tool = ShellTool::new();
        assert_eq!(tool.name(), "shell");
    }

    #[test]
    fn shell_tool_description() {
        let tool = ShellTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn shell_tool_schema_has_command() {
        let tool = ShellTool::new();
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["command"].is_object());
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("command"))
        );
    }

    #[tokio::test]
    async fn shell_executes_allowed_command() {
        let tool = ShellTool::new();
        let ctx = ExecutionContext::test_default(test_security(AutonomyLevel::Supervised));
        let result = tool
            .execute(json!({"command": "echo hello"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(result.output.trim().contains("hello"));
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn shell_missing_command_param() {
        let tool = ShellTool::new();
        let ctx = ExecutionContext::test_default(test_security(AutonomyLevel::Supervised));
        let result = tool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("command"));
    }

    #[tokio::test]
    async fn shell_wrong_type_param() {
        let tool = ShellTool::new();
        let ctx = ExecutionContext::test_default(test_security(AutonomyLevel::Supervised));
        let result = tool.execute(json!({"command": 123}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn shell_captures_exit_code() {
        let tool = ShellTool::new();
        let ctx = ExecutionContext::test_default(test_security(AutonomyLevel::Supervised));
        let result = tool
            .execute(json!({"command": "ls /nonexistent_dir_xyz"}), &ctx)
            .await
            .unwrap();
        assert!(!result.success);
    }

    fn test_security_with_env_cmd() -> Arc<SecurityPolicy> {
        Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: std::env::temp_dir(),
            allowed_commands: vec!["env".into(), "echo".into()],
            ..SecurityPolicy::default()
        })
    }

    /// RAII guard that restores an environment variable to its original state on drop,
    /// ensuring cleanup even if the test panics.
    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            // SAFETY: This helper is used only in test code to set process env vars.
            // Tests that use EnvGuard run on the current-thread Tokio runtime and this
            // guard restores the original value on drop, keeping access scoped.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                // SAFETY: Test-only restoration of a variable that was previously read by
                // this guard, returning process state to its original value.
                Some(val) => unsafe {
                    std::env::set_var(self.key, val);
                },
                // SAFETY: Test-only cleanup for variables introduced by EnvGuard::set.
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shell_does_not_leak_api_key() {
        let _g1 = EnvGuard::set("API_KEY", "sk-test-secret-12345");
        let _g2 = EnvGuard::set("ASTERONIRIS_API_KEY", "sk-test-secret-67890");

        let tool = ShellTool::new();
        let ctx = ExecutionContext::test_default(test_security_with_env_cmd());
        let result = tool.execute(json!({"command": "env"}), &ctx).await.unwrap();
        assert!(result.success);
        assert!(
            !result.output.contains("sk-test-secret-12345"),
            "API_KEY leaked to shell command output"
        );
        assert!(
            !result.output.contains("sk-test-secret-67890"),
            "ASTERONIRIS_API_KEY leaked to shell command output"
        );
    }

    #[tokio::test]
    async fn shell_rejects_disallowed_command() {
        let tool = ShellTool::new();
        // Create a policy that only allows "echo"
        let security = Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: std::env::temp_dir(),
            allowed_commands: vec!["echo".into()],
            ..SecurityPolicy::default()
        });
        let ctx = ExecutionContext::test_default(security);
        let result = tool
            .execute(json!({"command": "rm -rf /"}), &ctx)
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

    #[tokio::test]
    async fn shell_rejects_subshell_operator() {
        let tool = ShellTool::new();
        let security = Arc::new(SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            workspace_dir: std::env::temp_dir(),
            allowed_commands: vec!["echo".into()],
            ..SecurityPolicy::default()
        });
        let ctx = ExecutionContext::test_default(security);
        let result = tool
            .execute(json!({"command": "echo $(whoami)"}), &ctx)
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

    #[tokio::test]
    async fn shell_preserves_path_and_home() {
        let tool = ShellTool::new();
        let ctx = ExecutionContext::test_default(test_security_with_env_cmd());

        let result = tool
            .execute(json!({"command": "echo $HOME"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            !result.output.trim().is_empty(),
            "HOME should be available in shell"
        );

        let result = tool
            .execute(json!({"command": "echo $PATH"}), &ctx)
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            !result.output.trim().is_empty(),
            "PATH should be available in shell"
        );
    }
}
