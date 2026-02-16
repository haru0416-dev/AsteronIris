use super::traits::RuntimeAdapter;
use std::path::PathBuf;

pub struct DockerRuntime;

impl DockerRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl RuntimeAdapter for DockerRuntime {
    fn name(&self) -> &str {
        "docker"
    }

    fn has_shell_access(&self) -> bool {
        true
    }

    fn has_filesystem_access(&self) -> bool {
        true
    }

    fn storage_path(&self) -> PathBuf {
        PathBuf::from("/workspace/.asteroniris")
    }

    fn supports_long_running(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_name() {
        assert_eq!(DockerRuntime::new().name(), "docker");
    }

    #[test]
    fn docker_has_shell_access() {
        assert!(DockerRuntime::new().has_shell_access());
    }

    #[test]
    fn docker_has_filesystem_access() {
        assert!(DockerRuntime::new().has_filesystem_access());
    }

    #[test]
    fn docker_supports_long_running() {
        assert!(DockerRuntime::new().supports_long_running());
    }

    #[test]
    fn docker_memory_budget_unlimited() {
        assert_eq!(DockerRuntime::new().memory_budget(), 0);
    }

    #[test]
    fn docker_storage_path_is_workspace_scoped() {
        let path = DockerRuntime::new().storage_path();
        assert_eq!(path, PathBuf::from("/workspace/.asteroniris"));
    }
}
