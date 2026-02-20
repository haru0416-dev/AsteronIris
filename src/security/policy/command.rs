use super::SecurityPolicy;
use super::types::AutonomyLevel;

/// Skip leading environment variable assignments (e.g. `FOO=bar cmd args`).
/// Returns the remainder starting at the first non-assignment word.
fn skip_env_assignments(s: &str) -> &str {
    let mut rest = s;
    loop {
        let Some(word) = rest.split_whitespace().next() else {
            return rest;
        };
        if word.contains('=')
            && word
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        {
            rest = rest[word.len()..].trim_start();
        } else {
            return rest;
        }
    }
}

/// Git-specific config keys that enable arbitrary code execution when passed
/// via `git -c <key>=<val>` or `git clone --config <key>=<val>`.
const GIT_BLOCKED_CONFIG_KEYS: &[&str] = &[
    "core.sshcommand",
    "core.fsmonitor",
    "core.pager",
    "core.editor",
    "core.askpass",
    "credential.",
    "diff.external",
    "merge.tool",
    "filter.",
];

fn is_git_config_injection(args: &str) -> bool {
    let lower = args.to_lowercase();
    // `git -c <key>=<val>` or `git clone --config <key>=<val>`
    if !lower.contains("-c ") && !lower.contains("--config ") && !lower.contains("--config=") {
        return false;
    }
    GIT_BLOCKED_CONFIG_KEYS
        .iter()
        .any(|key| lower.contains(key))
}

fn has_blocked_arguments(base_cmd: &str, full_segment: &str, allowed_commands: &[String]) -> bool {
    let args = full_segment
        .trim()
        .strip_prefix(base_cmd)
        .unwrap_or("")
        .trim_start();

    let words: Vec<&str> = args.split_whitespace().collect();
    let subcommand = words.first().copied().unwrap_or("");

    match base_cmd {
        "git" => {
            // Network egress
            if matches!(subcommand, "push" | "send-email" | "request-pull") {
                return true;
            }
            // Credential theft
            if subcommand == "credential" {
                return true;
            }
            // Remote mutation (allow read-only: -v, show, get-url)
            if subcommand == "remote" {
                let sub_action = words.get(1).copied().unwrap_or("");
                return !matches!(sub_action, "" | "-v" | "show" | "get-url");
            }
            // Config: allow reads, block writes and --global/--system
            if subcommand == "config" {
                let has_write_flag = words.iter().any(|w| matches!(*w, "--global" | "--system"));
                let config_args: Vec<_> = words
                    .iter()
                    .skip(1)
                    .filter(|w| !w.starts_with('-'))
                    .collect();
                return has_write_flag || config_args.len() > 1;
            }
            // Submodule: block only `add` (pulls from external URL)
            if subcommand == "submodule" {
                let sub_action = words.get(1).copied().unwrap_or("");
                return sub_action == "add";
            }
            // Protocol-level code execution
            if words.iter().any(|w| {
                *w == "--upload-pack"
                    || w.starts_with("--upload-pack=")
                    || *w == "--receive-pack"
                    || w.starts_with("--receive-pack=")
            }) {
                return true;
            }
            // Config injection via -c / --config
            if is_git_config_injection(args) {
                return true;
            }
            false
        }
        "npm" => matches!(
            subcommand,
            "publish" | "login" | "adduser" | "owner" | "token" | "access" | "profile"
        ),
        "cargo" => matches!(subcommand, "publish" | "login" | "owner" | "yank"),
        "find" => {
            if words.contains(&"-delete") {
                return true;
            }
            // `-exec`/`-execdir`: validate that the exec'd command is in the allowlist
            for (i, w) in words.iter().enumerate() {
                if (*w == "-exec" || *w == "-execdir")
                    && let Some(exec_cmd) = words.get(i + 1)
                {
                    let exec_base = exec_cmd.rsplit('/').next().unwrap_or(exec_cmd);
                    if !allowed_commands.iter().any(|a| a == exec_base) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

impl SecurityPolicy {
    /// Check if a shell command is allowed.
    ///
    /// Validates the **entire** command string, not just the first word:
    /// - Blocks subshell operators (`` ` ``, `$(`) that hide arbitrary execution
    /// - Splits on command separators (`|`, `&&`, `||`, `;`, newlines) and
    ///   validates each sub-command against the allowlist
    /// - Blocks output redirections (`>`, `>>`) that could write outside workspace
    /// - Blocks dangerous arguments/subcommands that enable code execution,
    ///   network egress, or credential access
    pub fn is_command_allowed(&self, command: &str) -> bool {
        if self.autonomy == AutonomyLevel::ReadOnly {
            return false;
        }

        // Block subshell/expansion operators — these allow hiding arbitrary
        // commands inside an allowed command (e.g. `echo $(rm -rf /)`)
        if command.contains('`') || command.contains("$(") || command.contains("${") {
            return false;
        }

        // Block output redirections — they can write to arbitrary paths
        if command.contains('>') {
            return false;
        }

        // Split on command separators and validate each sub-command.
        let mut normalized = command.to_string();
        for sep in ["&&", "||"] {
            normalized = normalized.replace(sep, "\x00");
        }
        for sep in ['\n', ';', '|'] {
            normalized = normalized.replace(sep, "\x00");
        }

        for segment in normalized.split('\x00') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }

            let cmd_part = skip_env_assignments(segment);

            let base_cmd = cmd_part
                .split_whitespace()
                .next()
                .unwrap_or("")
                .rsplit('/')
                .next()
                .unwrap_or("");

            if base_cmd.is_empty() {
                continue;
            }

            if !self
                .allowed_commands
                .iter()
                .any(|allowed| allowed == base_cmd)
            {
                return false;
            }

            if has_blocked_arguments(base_cmd, cmd_part, &self.allowed_commands) {
                return false;
            }
        }

        // At least one command must be present
        normalized.split('\x00').any(|s| {
            let s = skip_env_assignments(s.trim());
            s.split_whitespace().next().is_some_and(|w| !w.is_empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AutonomyLevel, SecurityPolicy, has_blocked_arguments, is_git_config_injection,
        skip_env_assignments,
    };

    fn policy_with_allowed(allowed_commands: &[&str]) -> SecurityPolicy {
        SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            allowed_commands: allowed_commands.iter().map(ToString::to_string).collect(),
            ..SecurityPolicy::default()
        }
    }

    #[test]
    fn skip_env_assignments_strips_single_assignment() {
        assert_eq!(skip_env_assignments("VAR=value cmd"), "cmd");
    }

    #[test]
    fn skip_env_assignments_strips_multiple_assignments() {
        assert_eq!(
            skip_env_assignments("VAR1=a VAR2=b cmd --flag"),
            "cmd --flag"
        );
    }

    #[test]
    fn skip_env_assignments_keeps_plain_command() {
        assert_eq!(skip_env_assignments("cmd"), "cmd");
    }

    #[test]
    fn skip_env_assignments_handles_empty_input() {
        assert_eq!(skip_env_assignments(""), "");
    }

    #[test]
    fn is_git_config_injection_ignores_normal_git_commands() {
        assert!(!is_git_config_injection("status"));
        assert!(!is_git_config_injection("log --oneline"));
        assert!(!is_git_config_injection("diff --name-only"));
    }

    #[test]
    fn is_git_config_injection_handles_dash_c_patterns() {
        assert!(!is_git_config_injection("-c user.email=evil@hack"));
        assert!(is_git_config_injection("-c core.sshCommand=sh"));
    }

    #[test]
    fn is_git_config_injection_detects_config_flags_in_various_positions() {
        assert!(is_git_config_injection("clone --config core.pager=sh repo"));
        assert!(is_git_config_injection(
            "clone --config=core.editor=sh repo"
        ));
        assert!(is_git_config_injection("status -c credential.helper=!sh"));
    }

    #[test]
    fn has_blocked_arguments_git_dangerous_subcommands_are_blocked() {
        assert!(has_blocked_arguments("git", "git push", &[]));
        assert!(has_blocked_arguments("git", "git send-email", &[]));
        assert!(has_blocked_arguments("git", "git request-pull", &[]));
        assert!(has_blocked_arguments("git", "git remote add origin x", &[]));
        assert!(has_blocked_arguments(
            "git",
            "git remote set-url origin x",
            &[]
        ));
        assert!(has_blocked_arguments(
            "git",
            "git config --global user.name x",
            &[]
        ));
        assert!(has_blocked_arguments("git", "git credential fill", &[]));
    }

    #[test]
    fn has_blocked_arguments_git_safe_subcommands_are_allowed() {
        assert!(!has_blocked_arguments("git", "git status", &[]));
        assert!(!has_blocked_arguments("git", "git log", &[]));
        assert!(!has_blocked_arguments("git", "git diff", &[]));
    }

    #[test]
    fn has_blocked_arguments_npm_rules() {
        assert!(has_blocked_arguments("npm", "npm publish", &[]));
        assert!(!has_blocked_arguments("npm", "npm list", &[]));
        assert!(!has_blocked_arguments("npm", "npm run test", &[]));
    }

    #[test]
    fn has_blocked_arguments_cargo_rules() {
        assert!(has_blocked_arguments("cargo", "cargo publish", &[]));
        assert!(!has_blocked_arguments("cargo", "cargo build", &[]));
        assert!(!has_blocked_arguments("cargo", "cargo test", &[]));
    }

    #[test]
    fn has_blocked_arguments_pip_is_unrestricted_by_this_filter() {
        assert!(!has_blocked_arguments("pip", "pip install foo", &[]));
        assert!(!has_blocked_arguments("pip", "pip uninstall foo", &[]));
        assert!(!has_blocked_arguments("pip", "pip list", &[]));
    }

    #[test]
    fn is_command_allowed_accepts_safe_allowed_commands() {
        let policy = policy_with_allowed(&["git", "npm", "cargo", "pip"]);
        assert!(policy.is_command_allowed("git status"));
        assert!(policy.is_command_allowed("npm run test"));
        assert!(policy.is_command_allowed("cargo test"));
    }

    #[test]
    fn is_command_allowed_rejects_blocked_arguments() {
        let policy = policy_with_allowed(&["git", "npm", "cargo"]);
        assert!(!policy.is_command_allowed("git push"));
        assert!(!policy.is_command_allowed("git -c core.sshCommand=sh status"));
        assert!(!policy.is_command_allowed("npm publish"));
        assert!(!policy.is_command_allowed("cargo publish"));
    }

    #[test]
    fn is_command_allowed_rejects_empty_and_whitespace_only_commands() {
        let policy = policy_with_allowed(&["git"]);
        assert!(!policy.is_command_allowed(""));
        assert!(!policy.is_command_allowed("   \t  \n  "));
    }

    #[test]
    fn is_command_allowed_handles_extra_whitespace_and_env_prefixes() {
        let policy = policy_with_allowed(&["git"]);
        assert!(policy.is_command_allowed("  VAR=a   git   status   "));
    }

    #[test]
    fn is_command_allowed_is_case_sensitive() {
        let policy = policy_with_allowed(&["git"]);
        assert!(policy.is_command_allowed("git status"));
        assert!(!policy.is_command_allowed("Git status"));
    }

    #[test]
    fn is_command_allowed_rejects_subshell_expansion_and_redirection() {
        let policy = policy_with_allowed(&["echo"]);
        assert!(!policy.is_command_allowed("echo $(whoami)"));
        assert!(!policy.is_command_allowed("echo hi > out.txt"));
    }

    #[test]
    fn is_command_allowed_rejects_mixed_segments_with_one_disallowed_command() {
        let policy = policy_with_allowed(&["git", "echo"]);
        assert!(policy.is_command_allowed("git status && echo ok"));
        assert!(!policy.is_command_allowed("git status && curl https://example.com"));
    }

    #[test]
    fn is_command_allowed_denies_all_in_read_only_mode() {
        let mut policy = policy_with_allowed(&["git"]);
        policy.autonomy = AutonomyLevel::ReadOnly;
        assert!(!policy.is_command_allowed("git status"));
    }
}
