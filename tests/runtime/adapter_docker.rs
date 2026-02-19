use asteroniris::config::RuntimeConfig;
use asteroniris::runtime::create_runtime;

#[test]
fn docker_runtime_contract_is_gated_by_default() {
    let config = RuntimeConfig {
        kind: "docker".to_string(),
        enable_docker_runtime: false,
    };

    let message = match create_runtime(&config) {
        Ok(_) => panic!("docker runtime should stay gated until rollout is enabled"),
        Err(error) => error.to_string(),
    };

    assert!(message.contains("runtime.kind='docker'"));
    assert!(message.contains("disabled by rollout gate"));
    assert!(message.contains("runtime.enable_docker_runtime=true"));
}

#[test]
fn docker_runtime_contract_has_native_parity_for_supported_capabilities() {
    let native = create_runtime(&RuntimeConfig {
        kind: "native".to_string(),
        enable_docker_runtime: false,
    })
    .expect("native runtime should be created");
    let docker = create_runtime(&RuntimeConfig {
        kind: "docker".to_string(),
        enable_docker_runtime: true,
    })
    .expect("docker runtime should be created when rollout gate is enabled");

    assert_eq!(docker.name(), "docker");
    assert_eq!(docker.has_shell_access(), native.has_shell_access());
    assert_eq!(
        docker.has_filesystem_access(),
        native.has_filesystem_access()
    );
    assert_eq!(
        docker.supports_long_running(),
        native.supports_long_running()
    );
    assert_eq!(docker.memory_budget(), native.memory_budget());

    let docker_storage_path = docker.storage_path();
    assert!(
        docker_storage_path
            .to_string_lossy()
            .contains("asteroniris"),
        "docker storage path should include asteroniris, got: {}",
        docker_storage_path.display()
    );
}

#[test]
fn docker_runtime_error_remains_distinct_from_unknown_and_empty_runtime_errors() {
    let docker_error = match create_runtime(&RuntimeConfig {
        kind: "docker".to_string(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("docker should fail while rollout gate is disabled"),
        Err(error) => error.to_string(),
    };

    let unknown_error = match create_runtime(&RuntimeConfig {
        kind: "totally-unknown-runtime".to_string(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("unknown runtime should fail with unknown-kind message"),
        Err(error) => error.to_string(),
    };

    let empty_error = match create_runtime(&RuntimeConfig {
        kind: String::new(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("empty runtime should fail with empty-kind message"),
        Err(error) => error.to_string(),
    };

    assert!(docker_error.contains("disabled by rollout gate"));
    assert!(unknown_error.contains("Unknown runtime kind"));
    assert!(empty_error.contains("cannot be empty"));
    assert_ne!(docker_error, unknown_error);
    assert_ne!(docker_error, empty_error);
}
