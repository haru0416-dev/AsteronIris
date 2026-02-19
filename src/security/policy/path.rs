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
