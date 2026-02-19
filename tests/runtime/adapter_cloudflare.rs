use asteroniris::config::RuntimeConfig;
use asteroniris::runtime::{
    create_runtime, CLOUDFLARE_UNSUPPORTED_MESSAGE, DOCKER_ROLLOUT_GATE_MESSAGE,
};

#[test]
fn runtime_cloudflare_explicitly_unsupported() {
    let error_message = match create_runtime(&RuntimeConfig {
        kind: "cloudflare".to_string(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("cloudflare runtime must remain explicitly unsupported in this cycle"),
        Err(error) => error.to_string(),
    };

    assert_eq!(error_message, CLOUDFLARE_UNSUPPORTED_MESSAGE);
    assert!(error_message.contains("explicitly unsupported"));
    assert!(error_message.contains("runtime.kind='native'"));
    assert!(error_message.contains("no runtime fallback"));
}

#[test]
fn cloudflare_error_contract_stays_distinct_from_docker_gate_unknown_and_empty_kinds() {
    let cloudflare_error = match create_runtime(&RuntimeConfig {
        kind: "cloudflare".to_string(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("cloudflare runtime must remain unsupported"),
        Err(error) => error.to_string(),
    };

    let docker_error = match create_runtime(&RuntimeConfig {
        kind: "docker".to_string(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("docker runtime should remain rollout-gated by default"),
        Err(error) => error.to_string(),
    };

    let unknown_error = match create_runtime(&RuntimeConfig {
        kind: "runtime-not-real".to_string(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("unknown runtime should be rejected"),
        Err(error) => error.to_string(),
    };

    let empty_error = match create_runtime(&RuntimeConfig {
        kind: String::new(),
        enable_docker_runtime: false,
    }) {
        Ok(_) => panic!("empty runtime kind should be rejected"),
        Err(error) => error.to_string(),
    };

    assert_eq!(cloudflare_error, CLOUDFLARE_UNSUPPORTED_MESSAGE);
    assert_eq!(docker_error, DOCKER_ROLLOUT_GATE_MESSAGE);
    assert_eq!(
        unknown_error,
        "Unknown runtime kind 'runtime-not-real'. Supported values: native, docker"
    );
    assert_eq!(
        empty_error,
        "runtime.kind cannot be empty. Supported values: native, docker"
    );

    assert_ne!(cloudflare_error, docker_error);
    assert_ne!(cloudflare_error, unknown_error);
    assert_ne!(cloudflare_error, empty_error);
    assert_ne!(docker_error, unknown_error);
    assert_ne!(docker_error, empty_error);
    assert_ne!(unknown_error, empty_error);
}

#[test]
fn cloudflare_runtime_never_falls_back_to_native_or_docker_paths() {
    for enable_docker_runtime in [false, true] {
        let cloudflare_result = create_runtime(&RuntimeConfig {
            kind: "cloudflare".to_string(),
            enable_docker_runtime,
        });

        let error_message = match cloudflare_result {
            Ok(runtime) => panic!(
                "cloudflare runtime must not fallback to {} when enable_docker_runtime={enable_docker_runtime}",
                runtime.name()
            ),
            Err(error) => error.to_string(),
        };

        assert_eq!(error_message, CLOUDFLARE_UNSUPPORTED_MESSAGE);
        assert!(error_message.contains("no runtime fallback"));
    }
}
