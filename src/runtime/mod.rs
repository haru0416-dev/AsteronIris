pub mod docker;
pub mod native;
pub mod traits;

pub use docker::DockerRuntime;
pub use native::NativeRuntime;
pub use traits::RuntimeAdapter;

use crate::config::{RuntimeConfig, RuntimeKind};

pub const DOCKER_ROLLOUT_GATE_MESSAGE: &str =
    "runtime.kind='docker' is disabled by rollout gate. Set runtime.enable_docker_runtime=true to enable experimental docker runtime.";

/// Factory: create the right runtime from config
pub fn create_runtime(config: &RuntimeConfig) -> anyhow::Result<Box<dyn RuntimeAdapter>> {
    match config.kind {
        RuntimeKind::Native => Ok(Box::new(NativeRuntime::new())),
        RuntimeKind::Docker => {
            if config.enable_docker_runtime {
                Ok(Box::new(DockerRuntime::new()))
            } else {
                anyhow::bail!(DOCKER_ROLLOUT_GATE_MESSAGE)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_native() {
        let cfg = RuntimeConfig {
            kind: RuntimeKind::Native,
            enable_docker_runtime: false,
        };
        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "native");
        assert!(rt.has_shell_access());
    }

    #[test]
    fn factory_docker_disabled_without_gate() {
        let cfg = RuntimeConfig {
            kind: RuntimeKind::Docker,
            enable_docker_runtime: false,
        };

        match create_runtime(&cfg) {
            Err(err) => {
                let message = err.to_string();
                assert!(message.contains("disabled by rollout gate"));
                assert!(message.contains("runtime.enable_docker_runtime=true"));
            }
            Ok(_) => panic!("docker runtime should be gated by default"),
        }
    }

    #[test]
    fn factory_docker_enabled_with_gate() {
        let cfg = RuntimeConfig {
            kind: RuntimeKind::Docker,
            enable_docker_runtime: true,
        };

        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "docker");
        assert!(rt.has_shell_access());
    }
}
