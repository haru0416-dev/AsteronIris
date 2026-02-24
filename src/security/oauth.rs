use anyhow::{Context, Result, bail};
use dialoguer::Password;
use directories::UserDirs;
use serde::Deserialize;
use std::fs;
use std::io::IsTerminal;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub struct OAuthImportCredential {
    pub target_provider: &'static str,
    pub source_name: &'static str,
    pub access_token: String,
}

/// Import a cached OAuth access token for a provider from local CLI credentials.
///
/// Supported providers:
/// - `openai`, `openai-codex`, `codex` (imports from `~/.codex/auth.json`)
/// - `anthropic`, `claude` (imports from env: `ASTERONIRIS_CLAUDE_SETUP_TOKEN` or `ANTHROPIC_OAUTH_TOKEN`)
pub fn import_oauth_access_token_for_provider(provider: &str) -> Result<Option<(String, String)>> {
    let imported = import_cached_oauth_credential(provider)?;

    Ok(imported.map(|credential| (credential.access_token, credential.source_name.to_string())))
}

/// Run OAuth setup/import for a provider and return the imported credential.
///
/// `provider` accepts aliases:
/// - `OpenAI`: `openai`, `openai-codex`, `codex`
/// - `Anthropic`: `anthropic`, `claude`
pub fn import_oauth_access_token_for_provider_with_login(
    provider: &str,
    skip_cli_login: bool,
    setup_token: Option<String>,
) -> Result<OAuthImportCredential> {
    let provider = OAuthProvider::parse(provider)?;
    match provider {
        OAuthProvider::Codex => import_codex_oauth(skip_cli_login),
        OAuthProvider::Claude => import_claude_oauth(skip_cli_login, setup_token),
    }
}

pub fn codex_login_status() -> Result<String> {
    let output = Command::new("codex")
        .args(["login", "status"])
        .output()
        .context("Failed to run `codex login status`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("`codex login status` returned non-zero status");
        }
        bail!("`codex login status` failed: {stderr}");
    }

    let status = select_preferred_status_text(&output.stdout, &output.stderr);
    if status.is_empty() {
        return Ok("available (no output from codex CLI)".to_string());
    }
    Ok(status)
}

pub fn claude_auth_status() -> Result<(bool, Option<String>)> {
    let output = Command::new("claude")
        .args(["auth", "status", "--json"])
        .output()
        .context("Failed to run `claude auth status --json`")?;

    if !output.status.success() {
        bail!("`claude auth status --json` returned non-zero status");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let status: ClaudeAuthStatus =
        serde_json::from_str(&stdout).context("Failed to parse claude auth status JSON")?;
    Ok((status.logged_in, status.auth_method))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OAuthProvider {
    Codex,
    Claude,
}

impl OAuthProvider {
    fn parse(input: &str) -> Result<Self> {
        let normalized = input.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "codex" | "openai" | "openai-codex" => Ok(Self::Codex),
            "claude" | "anthropic" => Ok(Self::Claude),
            _ => bail!(
                "Unsupported OAuth provider '{input}'. Use one of: codex, openai, openai-codex, claude, anthropic"
            ),
        }
    }
}

fn import_cached_oauth_credential(provider: &str) -> Result<Option<OAuthImportCredential>> {
    let provider = OAuthProvider::parse(provider)?;
    let imported = match provider {
        OAuthProvider::Codex => import_codex_oauth_cached(),
        OAuthProvider::Claude => import_claude_oauth_cached()?,
    };
    Ok(imported)
}

fn import_codex_oauth(skip_cli_login: bool) -> Result<OAuthImportCredential> {
    if !skip_cli_login {
        run_interactive_command("codex", &["login", "--device-auth"])?;
    }

    import_codex_oauth_cached().ok_or_else(|| {
        anyhow::anyhow!(
            "Codex OAuth import failed: access token was not found in ~/.codex/auth.json"
        )
    })
}

fn import_codex_oauth_cached() -> Option<OAuthImportCredential> {
    let Ok(auth_file) = load_codex_auth_file() else {
        return None;
    };

    let access_token = auth_file
        .tokens
        .as_ref()
        .and_then(|tokens| tokens.access_token.as_deref())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            auth_file
                .openai_api_key
                .as_deref()
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .map(ToOwned::to_owned)
        })?;

    Some(OAuthImportCredential {
        target_provider: "openai-codex",
        source_name: "codex",
        access_token,
    })
}

fn import_claude_oauth(
    skip_cli_login: bool,
    setup_token: Option<String>,
) -> Result<OAuthImportCredential> {
    if !skip_cli_login {
        run_interactive_command("claude", &["auth", "login"])?;
    }

    let env_setup_token = load_claude_setup_token_from_env();
    let has_non_interactive_token = setup_token.is_some() || env_setup_token.is_some();

    if !std::io::stdin().is_terminal() && !has_non_interactive_token {
        bail!(
            "Claude OAuth import in non-interactive mode requires --setup-token \
             or ANTHROPIC_OAUTH_TOKEN / ASTERONIRIS_CLAUDE_SETUP_TOKEN."
        );
    }

    let access_token = if let Some(token) = setup_token {
        normalize_claude_setup_token(&token)?
    } else if let Some(token) = env_setup_token {
        normalize_claude_setup_token(&token)?
    } else if let Some(token) = try_capture_claude_setup_token()? {
        token
    } else {
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

    Ok(OAuthImportCredential {
        target_provider: "anthropic",
        source_name: "claude",
        access_token,
    })
}

fn import_claude_oauth_cached() -> Result<Option<OAuthImportCredential>> {
    let setup_token = load_claude_setup_token_from_env();
    let Some(token) = setup_token else {
        return Ok(None);
    };

    let access_token = normalize_claude_setup_token(&token)?;

    Ok(Some(OAuthImportCredential {
        target_provider: "anthropic",
        source_name: "claude",
        access_token,
    }))
}

fn load_claude_setup_token_from_env() -> Option<String> {
    std::env::var("ASTERONIRIS_CLAUDE_SETUP_TOKEN")
        .ok()
        .or_else(|| std::env::var("ANTHROPIC_OAUTH_TOKEN").ok())
        .and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
}

fn normalize_claude_setup_token(token: &str) -> Result<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        bail!("Claude setup token cannot be empty");
    }
    if !trimmed.starts_with("sk-ant-oat01-") {
        bail!("Claude setup token must start with 'sk-ant-oat01-'");
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
    if !std::io::stdin().is_terminal() {
        bail!(
            "Cannot run interactive `{bin}` login in non-interactive mode. \
             Re-run in a terminal, or pass --skip-cli-login to import existing cached credentials."
        );
    }

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

fn extract_prefixed_token(text: &str, prefix: &str) -> Option<String> {
    let start = text.find(prefix)?;
    let content_start = start + prefix.len();
    let end = oauth_token_end(text, content_start);
    (end > content_start).then(|| text[start..end].to_string())
}

fn select_preferred_status_text(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout_text = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout_text.is_empty() {
        return stdout_text;
    }

    String::from_utf8_lossy(stderr).trim().to_string()
}

#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    #[serde(default)]
    tokens: Option<CodexAuthTokens>,
    #[serde(rename = "OPENAI_API_KEY", default)]
    openai_api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexAuthTokens {
    #[serde(default)]
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeAuthStatus {
    #[serde(rename = "loggedIn")]
    logged_in: bool,
    #[serde(rename = "authMethod")]
    auth_method: Option<String>,
}

fn load_codex_auth_file() -> Result<CodexAuthFile> {
    let home = UserDirs::new()
        .map(|user_dirs| user_dirs.home_dir().to_path_buf())
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
    fn normalize_claude_setup_token_rejects_non_setup_token() {
        let err = normalize_claude_setup_token("sk-ant-api-key").unwrap_err();
        assert!(err.to_string().contains("sk-ant-oat01"));
    }

    #[test]
    fn codex_auth_json_parses_access_token() {
        let parsed: CodexAuthFile = serde_json::from_str(
            r#"{
  "auth_mode": "chatgpt",
  "tokens": {
    "access_token": "acc-123"
  }
}"#,
        )
        .unwrap();

        let tokens = parsed.tokens.unwrap();
        assert_eq!(tokens.access_token.as_deref(), Some("acc-123"));
    }

    #[test]
    fn codex_auth_json_parses_openai_api_key_fallback() {
        let parsed: CodexAuthFile = serde_json::from_str(
            r#"{
  "auth_mode": "chatgpt",
  "OPENAI_API_KEY": "sk-openai-from-codex"
}"#,
        )
        .unwrap();

        assert_eq!(
            parsed.openai_api_key.as_deref(),
            Some("sk-openai-from-codex")
        );
    }

    #[test]
    fn extract_prefixed_token_finds_claude_setup_token() {
        let text = "token: sk-ant-oat01-abc123_DEF more";
        let token = extract_prefixed_token(text, "sk-ant-oat01-").unwrap();
        assert_eq!(token, "sk-ant-oat01-abc123_DEF");
    }

    #[test]
    fn select_preferred_status_text_uses_stdout_first() {
        let selected = select_preferred_status_text(b"stdout text", b"stderr text");
        assert_eq!(selected, "stdout text");
    }

    #[test]
    fn select_preferred_status_text_falls_back_to_stderr() {
        let selected = select_preferred_status_text(b"", b"stderr text");
        assert_eq!(selected, "stderr text");
    }
}
