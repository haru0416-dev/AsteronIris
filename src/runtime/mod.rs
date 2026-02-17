pub mod docker;
pub mod native;
pub mod traits;

pub use docker::DockerRuntime;
pub use native::NativeRuntime;
pub use traits::RuntimeAdapter;

use crate::config::RuntimeConfig;

pub const RUNTIME_SUPPORTED_VALUES: &str = "native, docker";
pub const DOCKER_ROLLOUT_GATE_MESSAGE: &str =
    "runtime.kind='docker' is disabled by rollout gate. Set runtime.enable_docker_runtime=true to enable experimental docker runtime.";
pub const CLOUDFLARE_UNSUPPORTED_MESSAGE: &str =
    "runtime.kind='cloudflare' is reserved and explicitly unsupported in this cycle; no runtime fallback will be used. Use runtime.kind='native' for now.";
pub const CLOUDFLARE_RESERVED_STATUS_MESSAGE: &str =
    "reserved/unsupported in this cycle (no fallback execution path)";

pub fn runtime_kind_contract_note(kind: &str) -> Option<&'static str> {
    match kind {
        "cloudflare" => Some(CLOUDFLARE_RESERVED_STATUS_MESSAGE),
        _ => None,
    }
}

/// Factory: create the right runtime from config
pub fn create_runtime(config: &RuntimeConfig) -> anyhow::Result<Box<dyn RuntimeAdapter>> {
    match config.kind.as_str() {
        "native" => Ok(Box::new(NativeRuntime::new())),
        "docker" => {
            if config.enable_docker_runtime {
                Ok(Box::new(DockerRuntime::new()))
            } else {
                anyhow::bail!(DOCKER_ROLLOUT_GATE_MESSAGE)
            }
        }
        "cloudflare" => anyhow::bail!(CLOUDFLARE_UNSUPPORTED_MESSAGE),
        other if other.trim().is_empty() => {
            anyhow::bail!(
                "runtime.kind cannot be empty. Supported values: {RUNTIME_SUPPORTED_VALUES}"
            )
        }
        other => {
            anyhow::bail!(
                "Unknown runtime kind '{other}'. Supported values: {RUNTIME_SUPPORTED_VALUES}"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_native() {
        let cfg = RuntimeConfig {
            kind: "native".into(),
            enable_docker_runtime: false,
        };
        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "native");
        assert!(rt.has_shell_access());
    }

    #[test]
    fn factory_docker_disabled_without_gate() {
        let cfg = RuntimeConfig {
            kind: "docker".into(),
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
            kind: "docker".into(),
            enable_docker_runtime: true,
        };

        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "docker");
        assert!(rt.has_shell_access());
    }

    #[test]
    fn factory_cloudflare_errors() {
        let cfg = RuntimeConfig {
            kind: "cloudflare".into(),
            enable_docker_runtime: false,
        };
        match create_runtime(&cfg) {
            Err(err) => {
                let message = err.to_string();
                assert_eq!(message, CLOUDFLARE_UNSUPPORTED_MESSAGE);
                assert!(message.contains("reserved and explicitly unsupported"));
                assert!(message.contains("no runtime fallback"));
            }
            Ok(_) => panic!("cloudflare runtime should error"),
        }
    }

    #[test]
    fn runtime_contract_note_cloudflare_is_explicit() {
        assert_eq!(
            runtime_kind_contract_note("cloudflare"),
            Some(CLOUDFLARE_RESERVED_STATUS_MESSAGE)
        );
        assert_eq!(runtime_kind_contract_note("native"), None);
        assert_eq!(runtime_kind_contract_note("docker"), None);
    }

    #[test]
    fn factory_unknown_errors() {
        let cfg = RuntimeConfig {
            kind: "wasm-edge-unknown".into(),
            enable_docker_runtime: false,
        };
        match create_runtime(&cfg) {
            Err(err) => assert_eq!(
                err.to_string(),
                "Unknown runtime kind 'wasm-edge-unknown'. Supported values: native, docker"
            ),
            Ok(_) => panic!("unknown runtime should error"),
        }
    }

    #[test]
    fn factory_empty_errors() {
        let cfg = RuntimeConfig {
            kind: String::new(),
            enable_docker_runtime: false,
        };
        match create_runtime(&cfg) {
            Err(err) => assert_eq!(
                err.to_string(),
                "runtime.kind cannot be empty. Supported values: native, docker"
            ),
            Ok(_) => panic!("empty runtime should error"),
        }
    }
}
