use crate::security::approval::{GrantScope, PermissionGrant};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Default)]
struct PermissionFile {
    #[serde(default)]
    grants: Vec<StoredGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredGrant {
    tool: String,
    pattern: String,
    scope: GrantScope,
    granted_at: String,
    granted_by: String,
}

#[derive(Debug)]
pub struct PermissionStore {
    session_grants: Mutex<Vec<PermissionGrant>>,
    permanent_grants: Mutex<Vec<PermissionGrant>>,
    permanent_records: Mutex<Vec<StoredGrant>>,
    entity_allowlists: Mutex<HashMap<String, HashSet<String>>>,
    store_path: PathBuf,
}

impl PermissionStore {
    pub fn load(workspace_dir: &Path) -> Self {
        let store_path = workspace_dir.join("permissions.toml");
        let permission_file = match fs::read_to_string(&store_path) {
            Ok(content) => {
                if content.trim().is_empty() {
                    PermissionFile::default()
                } else {
                    toml::from_str(&content).unwrap_or_else(|error| {
                        tracing::warn!(
                            path = %store_path.display(),
                            %error,
                            "failed to parse permissions.toml; starting with empty grants"
                        );
                        PermissionFile::default()
                    })
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let empty = PermissionFile::default();
                if let Err(write_error) = persist_permission_file(&store_path, &empty) {
                    tracing::warn!(
                        path = %store_path.display(),
                        %write_error,
                        "failed to initialize permissions.toml"
                    );
                }
                empty
            }
            Err(error) => {
                tracing::warn!(
                    path = %store_path.display(),
                    %error,
                    "failed to read permissions.toml; starting with empty grants"
                );
                PermissionFile::default()
            }
        };

        let permanent_grants = permission_file
            .grants
            .iter()
            .map(stored_to_permission_grant)
            .collect();

        Self {
            session_grants: Mutex::new(Vec::new()),
            permanent_grants: Mutex::new(permanent_grants),
            permanent_records: Mutex::new(permission_file.grants),
            entity_allowlists: Mutex::new(HashMap::new()),
            store_path,
        }
    }

    pub fn set_entity_allowlist(&self, entity_id: &str, allowlist: Option<HashSet<String>>) {
        let mut allowlists = self
            .entity_allowlists
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        match allowlist {
            Some(allowlist) => {
                allowlists.insert(entity_id.to_string(), allowlist);
            }
            None => {
                allowlists.remove(entity_id);
            }
        }
    }

    pub fn add_grant(&self, grant: PermissionGrant, entity_id: &str) -> Result<()> {
        anyhow::ensure!(
            !grant.tool.trim().is_empty(),
            "grant tool must not be empty"
        );
        anyhow::ensure!(
            !grant.pattern.trim().is_empty(),
            "grant pattern must not be empty"
        );

        if let Some(allowed_tools) = self
            .entity_allowlists
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(entity_id)
            .cloned()
            && !allowed_tools.contains(&grant.tool)
        {
            bail!(
                "cannot grant tool '{}' for entity '{}': tool not in allowlist",
                grant.tool,
                entity_id
            );
        }

        match grant.scope {
            GrantScope::Session => {
                self.session_grants
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(grant);
                Ok(())
            }
            GrantScope::Permanent => {
                let record = StoredGrant {
                    tool: grant.tool.clone(),
                    pattern: grant.pattern.clone(),
                    scope: GrantScope::Permanent,
                    granted_at: Utc::now().to_rfc3339(),
                    granted_by: entity_id.to_string(),
                };

                let mut records = self
                    .permanent_records
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut next_records = records.clone();
                next_records.push(record);
                persist_permission_file(
                    &self.store_path,
                    &PermissionFile {
                        grants: next_records.clone(),
                    },
                )?;

                *records = next_records;

                self.permanent_grants
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(grant);
                Ok(())
            }
        }
    }

    pub fn is_granted(&self, tool_name: &str, args_summary: &str) -> bool {
        let session_match = self
            .session_grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|grant| grant.tool == tool_name && pattern_matches(&grant.pattern, args_summary));

        if session_match {
            return true;
        }

        self.permanent_grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|grant| grant.tool == tool_name && pattern_matches(&grant.pattern, args_summary))
    }

    pub fn active_grants(&self) -> Vec<PermissionGrant> {
        let mut grants = self
            .session_grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        grants.extend(
            self.permanent_grants
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .cloned(),
        );
        grants
    }
}

#[must_use]
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(" *") {
        return value.starts_with(prefix)
            && value.len() > prefix.len()
            && value.as_bytes()[prefix.len()] == b' ';
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}

fn stored_to_permission_grant(grant: &StoredGrant) -> PermissionGrant {
    PermissionGrant {
        tool: grant.tool.clone(),
        pattern: grant.pattern.clone(),
        scope: grant.scope,
    }
}

fn persist_permission_file(path: &Path, data: &PermissionFile) -> Result<()> {
    let content = toml::to_string(data).context("failed to serialize permissions")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create permissions parent directory '{}'",
                parent.display()
            )
        })?;
    }

    fs::write(path, content)
        .with_context(|| format!("failed to write permissions file '{}'", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set permissions on '{}'", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn shell_grant(pattern: &str, scope: GrantScope) -> PermissionGrant {
        PermissionGrant {
            tool: "shell".to_string(),
            pattern: pattern.to_string(),
            scope,
        }
    }

    #[test]
    fn session_grant_works_within_session_and_clears_on_restart() {
        let tmp = TempDir::new().expect("tempdir");
        let store = PermissionStore::load(tmp.path());

        store
            .add_grant(shell_grant("cargo *", GrantScope::Session), "cli:local")
            .expect("add session grant");

        assert!(store.is_granted("shell", "cargo test"));

        let restarted = PermissionStore::load(tmp.path());
        assert!(!restarted.is_granted("shell", "cargo test"));
    }

    #[test]
    fn pattern_matching_prefix_space() {
        assert!(pattern_matches("cargo *", "cargo test"));
        assert!(!pattern_matches("cargo *", "python script.py"));
        assert!(!pattern_matches("cargo *", "cargo"));
    }

    #[test]
    fn pattern_matching_wildcard_everything() {
        assert!(pattern_matches("*", "cargo test"));
        assert!(pattern_matches("*", "anything"));
    }

    #[test]
    fn pattern_matching_exact_only() {
        assert!(pattern_matches("cargo test", "cargo test"));
        assert!(!pattern_matches("cargo test", "cargo test --lib"));
    }

    #[test]
    fn is_granted_false_when_no_grants() {
        let tmp = TempDir::new().expect("tempdir");
        let store = PermissionStore::load(tmp.path());
        assert!(!store.is_granted("shell", "cargo test"));
    }

    #[test]
    fn permanent_grant_serializes_and_deserializes_toml() {
        let tmp = TempDir::new().expect("tempdir");
        let store = PermissionStore::load(tmp.path());
        store
            .add_grant(shell_grant("cargo *", GrantScope::Permanent), "cli:local")
            .expect("add permanent grant");

        let file_content =
            fs::read_to_string(tmp.path().join("permissions.toml")).expect("read permissions");
        let parsed: PermissionFile = toml::from_str(&file_content).expect("parse permissions");

        assert_eq!(parsed.grants.len(), 1);
        assert_eq!(parsed.grants[0].tool, "shell");
        assert_eq!(parsed.grants[0].pattern, "cargo *");
        assert_eq!(parsed.grants[0].scope, GrantScope::Permanent);
        assert_eq!(parsed.grants[0].granted_by, "cli:local");
        assert!(!parsed.grants[0].granted_at.is_empty());
    }

    #[test]
    fn cannot_grant_tool_not_in_entity_allowlist() {
        let tmp = TempDir::new().expect("tempdir");
        let store = PermissionStore::load(tmp.path());
        store.set_entity_allowlist("entity:1", Some(HashSet::from(["file_read".to_string()])));

        let grant = PermissionGrant {
            tool: "shell".to_string(),
            pattern: "cargo *".to_string(),
            scope: GrantScope::Session,
        };

        assert!(store.add_grant(grant, "entity:1").is_err());
    }

    #[test]
    fn active_grants_returns_session_and_permanent() {
        let tmp = TempDir::new().expect("tempdir");
        let store = PermissionStore::load(tmp.path());

        store
            .add_grant(shell_grant("cargo test", GrantScope::Session), "cli:local")
            .expect("add session grant");
        store
            .add_grant(shell_grant("cargo *", GrantScope::Permanent), "cli:local")
            .expect("add permanent grant");

        let grants = store.active_grants();
        assert_eq!(grants.len(), 2);
        assert!(
            grants
                .iter()
                .any(|grant| grant.scope == GrantScope::Session && grant.pattern == "cargo test")
        );
        assert!(
            grants
                .iter()
                .any(|grant| grant.scope == GrantScope::Permanent && grant.pattern == "cargo *")
        );
    }
}
