use std::path::{Path, PathBuf};

use super::SecurityPolicy;

impl SecurityPolicy {
    /// Check if a file path is allowed (no path traversal, within workspace)
    pub fn is_path_allowed(&self, path: &str) -> bool {
        // Block null bytes (can truncate paths in C-backed syscalls)
        if path.contains('\0') {
            return false;
        }

        // Block path traversal: check for ".." as a path component
        if Path::new(path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return false;
        }

        // Block URL-encoded traversal attempts (e.g. ..%2f)
        let lower = path.to_lowercase();
        if lower.contains("..%2f") || lower.contains("%2f..") {
            return false;
        }

        // Expand tilde for comparison
        let expanded = if let Some(stripped) = path.strip_prefix("~/") {
            if let Some(home) = std::env::var("HOME").ok().map(PathBuf::from) {
                home.join(stripped).to_string_lossy().to_string()
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        };

        // Block absolute paths when workspace_only is set
        if self.workspace_only && Path::new(&expanded).is_absolute() {
            return false;
        }

        // Block forbidden paths using path-component-aware matching
        let expanded_path = Path::new(&expanded);
        for forbidden in &self.forbidden_paths {
            let forbidden_expanded = if let Some(stripped) = forbidden.strip_prefix("~/") {
                if let Some(home) = std::env::var("HOME").ok().map(PathBuf::from) {
                    home.join(stripped).to_string_lossy().to_string()
                } else {
                    forbidden.clone()
                }
            } else {
                forbidden.clone()
            };
            let forbidden_path = Path::new(&forbidden_expanded);
            if expanded_path.starts_with(forbidden_path) {
                return false;
            }
        }

        true
    }

    /// Validate that a resolved path is still inside the workspace.
    /// Call this AFTER joining `workspace_dir` + relative path and canonicalizing.
    pub fn is_resolved_path_allowed(&self, resolved: &Path) -> bool {
        // Must be under workspace_dir (prevents symlink escapes).
        // Prefer canonical workspace root so `/a/../b` style config paths don't
        // cause false positives or negatives.
        let workspace_root = self
            .workspace_dir
            .canonicalize()
            .unwrap_or_else(|_| self.workspace_dir.clone());
        resolved.starts_with(workspace_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AutonomyConfig;
    use std::fs;
    use tempfile::TempDir;

    fn policy_with(
        workspace: &Path,
        workspace_only: bool,
        forbidden_paths: Vec<String>,
    ) -> SecurityPolicy {
        SecurityPolicy {
            workspace_dir: workspace.to_path_buf(),
            workspace_only,
            forbidden_paths,
            ..SecurityPolicy::default()
        }
    }

    #[test]
    fn allows_normal_workspace_relative_paths() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(policy.is_path_allowed("src/main.rs"));
        assert!(policy.is_path_allowed("nested/dir/file.txt"));
    }

    #[test]
    fn blocks_parent_directory_traversal() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(!policy.is_path_allowed("../../etc/passwd"));
    }

    #[test]
    fn blocks_null_bytes() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(!policy.is_path_allowed("file\0.txt"));
    }

    #[test]
    fn blocks_url_encoded_traversal_variants() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(!policy.is_path_allowed("..%2f..%2fetc/passwd"));
        assert!(!policy.is_path_allowed("..%2F..%2Fetc/passwd"));
    }

    #[test]
    fn handles_tilde_paths_with_forbidden_prefix() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), false, vec!["~/.ssh".to_string()]);

        assert!(!policy.is_path_allowed("~/.ssh/id_rsa"));
    }

    #[test]
    fn blocks_absolute_paths_when_workspace_only_enabled() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(!policy.is_path_allowed("/tmp/file with spaces.txt"));
    }

    #[test]
    fn allows_absolute_paths_when_workspace_only_disabled_and_not_forbidden() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), false, vec!["/etc".to_string()]);

        assert!(policy.is_path_allowed("/my/project/data.txt"));
    }

    #[test]
    fn empty_path_and_space_paths_are_handled() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(policy.is_path_allowed(""));
        assert!(policy.is_path_allowed("folder with spaces/file name.txt"));
    }

    #[test]
    fn forbidden_matching_is_component_aware() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), false, vec!["/etc".to_string()]);

        assert!(!policy.is_path_allowed("/etc/shadow"));
        assert!(policy.is_path_allowed("/my/etc/shadow"));
    }

    #[test]
    fn symbolic_link_escape_requires_resolved_path_check() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), true, vec![]);

        assert!(policy.is_path_allowed("link_to_outside/secret.txt"));
    }

    #[test]
    fn default_autonomy_forbidden_paths_are_all_blocked() {
        let workspace = TempDir::new().expect("tempdir");
        let mut autonomy = AutonomyConfig::default();
        autonomy.workspace_only = false;
        let policy = SecurityPolicy::from_config(&autonomy, workspace.path());

        for forbidden in &autonomy.forbidden_paths {
            let candidate = format!("{forbidden}/sensitive.txt");
            assert!(
                !policy.is_path_allowed(&candidate),
                "forbidden path should be blocked: {forbidden}"
            );
        }
    }

    #[test]
    fn forbidden_paths_resist_simple_case_and_encoding_variants() {
        let workspace = TempDir::new().expect("tempdir");
        let policy = policy_with(workspace.path(), false, vec!["/etc".to_string()]);

        assert!(!policy.is_path_allowed("/etc/shadow"));
        assert!(!policy.is_path_allowed("/etc/%2e%2e/shadow"));
    }

    #[test]
    fn resolved_path_inside_workspace_is_allowed() {
        let workspace = TempDir::new().expect("tempdir");
        let nested = workspace.path().join("src");
        fs::create_dir_all(&nested).expect("create nested dir");
        let file_path = nested.join("main.rs");
        fs::write(&file_path, "fn main() {}\n").expect("write file");

        let policy = policy_with(workspace.path(), true, vec![]);
        let resolved = file_path.canonicalize().expect("canonicalize inside path");
        assert!(policy.is_resolved_path_allowed(&resolved));
    }

    #[test]
    fn resolved_path_outside_workspace_is_blocked() {
        let workspace = TempDir::new().expect("tempdir");
        let outside_root = TempDir::new().expect("tempdir");
        let outside_file = outside_root.path().join("escape.txt");
        fs::write(&outside_file, "escape\n").expect("write outside file");

        let policy = policy_with(workspace.path(), true, vec![]);
        let resolved = outside_file
            .canonicalize()
            .expect("canonicalize outside path");
        assert!(!policy.is_resolved_path_allowed(&resolved));
    }
}
