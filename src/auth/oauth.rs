use anyhow::{bail, Context, Result};
use dialoguer::Password;
use directories::UserDirs;
use serde::Deserialize;
use std::fs;
use std::io::IsTerminal;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OAuthProvider {
    Codex,
    Claude,
}

impl OAuthProvider {
    pub(super) fn parse(input: &str) -> Result<Self> {
        let normalized = input.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "codex" | "openai" | "openai-codex" => Ok(Self::Codex),
            "claude" | "anthropic" => Ok(Self::Claude),
            _ => bail!(
                "Unsupported OAuth provider '{input}'. Use one of: codex, openai, claude, anthropic"
            ),
        }
    }
}

pub(super) struct ImportedOAuthCredential {
    pub(super) target_provider: &'static str,
    pub(super) default_profile_id: &'static str,
    pub(super) default_label: &'static str,
    pub(super) source_name: &'static str,
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
}

pub(super) fn import_codex_oauth(skip_cli_login: bool) -> Result<ImportedOAuthCredential> {
    if !skip_cli_login {
        run_interactive_command("codex", &["login", "--device-auth"])?;
    }

    let auth_file = load_codex_auth_file()?;
    let tokens = auth_file
        .tokens
        .ok_or_else(|| anyhow::anyhow!("Codex auth.json missing tokens block"))?;

    let access_token = tokens
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Codex login succeeded but access token was not found"))?;

    let refresh_token = tokens
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(ToOwned::to_owned);

    Ok(ImportedOAuthCredential {
        target_provider: "openai",
        default_profile_id: "openai-codex-oauth-default",
        default_label: "OAuth (codex login)",
        source_name: "codex",
        access_token,
        refresh_token,
    })
}

pub(super) fn import_claude_oauth(
    skip_cli_login: bool,
    setup_token: Option<String>,
) -> Result<ImportedOAuthCredential> {
    if !skip_cli_login {
        run_interactive_command("claude", &["auth", "login"])?;
    }

    let access_token = if let Some(token) = setup_token {
        normalize_claude_setup_token(&token)?
    } else if let Some(token) = try_capture_claude_setup_token()? {
        token
    } else {
        if !std::io::stdin().is_terminal() {
            bail!(
                "Could not capture Claude setup token automatically in non-interactive mode. \
                 Re-run with --setup-token sk-ant-oat01-..."
            );
        }

        println!(
            "Could not auto-capture setup token. Please run `claude setup-token` in another terminal and paste it below."
        );
        let token = Password::new()
            .with_prompt("Claude setup token (input hidden)")
            .allow_empty_password(false)
            .interact()
            .context("Failed to read Claude setup token from terminal")?;
        normalize_claude_setup_token(&token)?
    };

    Ok(ImportedOAuthCredential {
        target_provider: "anthropic",
        default_profile_id: "anthropic-claude-oauth-default",
        default_label: "OAuth (claude login)",
        source_name: "claude",
        access_token,
        refresh_token: None,
    })
}

fn normalize_claude_setup_token(token: &str) -> Result<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        bail!("Claude setup token cannot be empty");
    }
    if !trimmed.starts_with("sk-ant-oat01-") {
        bail!("Claude setup token must start with 'sk-ant-oat01-'. Run `claude setup-token` first");
    }
    Ok(trimmed.to_string())
}

fn try_capture_claude_setup_token() -> Result<Option<String>> {
    let output = Command::new("claude")
        .arg("setup-token")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to run `claude setup-token`")?;

    let mut combined = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        combined.push('\n');
        combined.push_str(&stderr);
    }

    if !output.status.success() {
        return Ok(None);
    }

    Ok(extract_prefixed_token(&combined, "sk-ant-oat01-"))
}

fn run_interactive_command(bin: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(bin)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to run `{bin}`"))?;

    if !status.success() {
        bail!(
            "Command `{bin} {}` failed with status {status}",
            args.join(" ")
        );
    }

    Ok(())
}

fn is_oauth_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '+' | '/' | '=')
}

fn oauth_token_end(input: &str, from: usize) -> usize {
    let mut end = from;
    for (i, c) in input[from..].char_indices() {
        if is_oauth_token_char(c) {
            end = from + i + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

pub(super) fn extract_prefixed_token(text: &str, prefix: &str) -> Option<String> {
    let start = text.find(prefix)?;
    let content_start = start + prefix.len();
    let end = oauth_token_end(text, content_start);
    (end > content_start).then(|| text[start..end].to_string())
}

pub(super) fn codex_login_status() -> Result<String> {
    let output = Command::new("codex")
        .args(["login", "status"])
        .output()
        .context("Failed to run `codex login status`")?;

    if !output.status.success() {
        bail!("`codex login status` returned non-zero status");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

#[derive(Debug, Deserialize)]
pub(super) struct ClaudeAuthStatus {
    #[serde(rename = "loggedIn")]
    pub(super) logged_in: bool,
    #[serde(rename = "authMethod")]
    pub(super) auth_method: Option<String>,
}

pub(super) fn claude_auth_status() -> Result<ClaudeAuthStatus> {
    let output = Command::new("claude")
        .args(["auth", "status", "--json"])
        .output()
        .context("Failed to run `claude auth status --json`")?;

    if !output.status.success() {
        bail!("`claude auth status --json` returned non-zero status");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).context("Failed to parse claude auth status JSON")
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexAuthFile {
    #[serde(default)]
    pub(super) tokens: Option<CodexAuthTokens>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CodexAuthTokens {
    #[serde(default)]
    pub(super) access_token: Option<String>,
    #[serde(default)]
    pub(super) refresh_token: Option<String>,
}

pub(super) fn load_codex_auth_file() -> Result<CodexAuthFile> {
    let home = UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not resolve home directory")?;
    let path = home.join(".codex").join("auth.json");

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read Codex auth file: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse Codex auth file: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_prefixed_token_finds_claude_setup_token() {
        let text = "token: sk-ant-oat01-abc123_DEF more";
        let token = extract_prefixed_token(text, "sk-ant-oat01-").unwrap();
        assert_eq!(token, "sk-ant-oat01-abc123_DEF");
    }

    #[test]
    fn normalize_claude_setup_token_rejects_non_setup_token() {
        let err = normalize_claude_setup_token("sk-ant-api-key").unwrap_err();
        assert!(err.to_string().contains("sk-ant-oat01"));
    }

    #[test]
    fn codex_auth_json_parses_access_and_refresh_tokens() {
        let parsed: CodexAuthFile = serde_json::from_str(
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "acc-123",
    "refresh_token": "ref-456"
  }
}"#,
        )
        .unwrap();

        let tokens = parsed.tokens.unwrap();
        assert_eq!(tokens.access_token.as_deref(), Some("acc-123"));
        assert_eq!(tokens.refresh_token.as_deref(), Some("ref-456"));
    }
}
