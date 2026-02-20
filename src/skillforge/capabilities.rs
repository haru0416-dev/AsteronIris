//! Deno-style capability declarations for skills.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Permission declarations a skill can request.
/// `None` = denied, `Some(vec![])` = denied (empty allowlist), `Some(vec!["..."])` = specific grants.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillPermissions {
    /// Network access. `None` = deny all. `Some(["host:port", ...])` = allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net: Option<Vec<String>>,
    /// Filesystem read. `None` = deny all. `Some(["path", ...])` = allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<Vec<PathBuf>>,
    /// Filesystem write. `None` = deny all. `Some(["path", ...])` = allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<Vec<PathBuf>>,
    /// Environment variable access. `None` = deny all. `Some(["VAR", ...])` = allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
    /// Subprocess execution. `None` = deny all. `Some(["cmd", ...])` = allowlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Vec<String>>,
    /// Foreign function interface. Almost always false.
    #[serde(default)]
    pub ffi: bool,
    /// System info access. `None` = deny all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys: Option<Vec<String>>,
}

impl SkillPermissions {
    /// Returns permissions that deny everything.
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Returns true if no permissions are granted at all.
    pub fn is_empty(&self) -> bool {
        self.net.is_none()
            && self.read.is_none()
            && self.write.is_none()
            && self.env.is_none()
            && self.run.is_none()
            && !self.ffi
            && self.sys.is_none()
    }

    /// Check if network access is requested.
    pub fn requests_net(&self) -> bool {
        self.net.as_ref().is_some_and(|v| !v.is_empty())
    }

    /// Check if env access is requested.
    pub fn requests_env(&self) -> bool {
        self.env.as_ref().is_some_and(|v| !v.is_empty())
    }

    /// Check if subprocess execution is requested.
    pub fn requests_run(&self) -> bool {
        self.run.as_ref().is_some_and(|v| !v.is_empty())
    }

    /// Check if filesystem write is requested.
    pub fn requests_write(&self) -> bool {
        self.write.as_ref().is_some_and(|v| !v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn deny_all_is_empty() {
        let perms = SkillPermissions::deny_all();
        assert!(perms.is_empty());
        assert!(!perms.requests_net());
        assert!(!perms.requests_env());
        assert!(!perms.requests_run());
        assert!(!perms.requests_write());
    }

    #[test]
    fn default_is_deny_all() {
        let perms = SkillPermissions::default();
        assert!(perms.is_empty());
    }

    #[test]
    fn net_permission_detected() {
        let perms = SkillPermissions {
            net: Some(vec!["api.example.com:443".into()]),
            ..Default::default()
        };
        assert!(!perms.is_empty());
        assert!(perms.requests_net());
        assert!(!perms.requests_env());
    }

    #[test]
    fn env_permission_detected() {
        let perms = SkillPermissions {
            env: Some(vec!["HOME".into()]),
            ..Default::default()
        };
        assert!(perms.requests_env());
        assert!(!perms.requests_net());
    }

    #[test]
    fn run_permission_detected() {
        let perms = SkillPermissions {
            run: Some(vec!["python3".into()]),
            ..Default::default()
        };
        assert!(perms.requests_run());
    }

    #[test]
    fn write_permission_detected() {
        let perms = SkillPermissions {
            write: Some(vec![PathBuf::from("/tmp/output")]),
            ..Default::default()
        };
        assert!(perms.requests_write());
    }

    #[test]
    fn empty_vec_not_considered_a_request() {
        let perms = SkillPermissions {
            net: Some(vec![]),
            env: Some(vec![]),
            run: Some(vec![]),
            write: Some(vec![]),
            ..Default::default()
        };
        assert!(!perms.requests_net());
        assert!(!perms.requests_env());
        assert!(!perms.requests_run());
        assert!(!perms.requests_write());
        assert!(!perms.is_empty());
    }

    #[test]
    fn ffi_makes_not_empty() {
        let perms = SkillPermissions {
            ffi: true,
            ..Default::default()
        };
        assert!(!perms.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let perms = SkillPermissions {
            net: Some(vec!["example.com:443".into()]),
            read: Some(vec![PathBuf::from("/data")]),
            env: Some(vec!["API_KEY".into()]),
            ..Default::default()
        };
        let json = serde_json::to_string(&perms).unwrap();
        let back: SkillPermissions = serde_json::from_str(&json).unwrap();
        assert_eq!(back.net, perms.net);
        assert_eq!(back.read, perms.read);
        assert_eq!(back.env, perms.env);
        assert!(back.write.is_none());
    }

    #[test]
    fn skip_serializing_none_fields() {
        let perms = SkillPermissions::deny_all();
        let json = serde_json::to_string(&perms).unwrap();
        assert!(!json.contains("net"));
        assert!(!json.contains("read"));
        assert!(!json.contains("write"));
        assert!(!json.contains("env"));
        assert!(!json.contains("run"));
        assert!(!json.contains("sys"));
        assert!(json.contains("ffi"));
    }
}
