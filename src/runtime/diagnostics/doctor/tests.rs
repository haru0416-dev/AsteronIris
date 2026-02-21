use super::{autonomy_governance_lines, memory_rollout_lines};
use crate::config::Config;

#[test]
fn doctor_reports_autonomy_gates() {
    let mut config = Config::default();
    config.observability.backend = "prometheus".into();

    let lines = autonomy_governance_lines(&config);

    assert!(
        lines
            .iter()
            .any(|line| line.contains("external actions") && line.contains("disabled"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("temperature band") && line.contains('['))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("rollout stage") && line.contains("off"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("rollout policy") && line.contains("enabled=off"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("observability backend") && line.contains("prometheus"))
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("autonomy lifecycle metrics") && line.contains("enabled")
        })
    );
}

#[test]
fn doctor_reports_memory_rollout() {
    let mut config = Config::default();
    config.memory.backend = "sqlite".into();
    config.memory.auto_save = true;
    config.autonomy.rollout.enabled = true;

    let snapshot = serde_json::json!({
        "memory_rollout": {
            "consolidation": "healthy",
            "conflict": "healthy",
            "revocation": "healthy",
            "governance": "healthy"
        }
    });

    let lines = memory_rollout_lines(&config, &snapshot);

    assert!(lines.iter().any(|line| {
        line.contains("memory backend: sqlite") && line.contains("consolidation=on")
    }));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("revocation=supported")
                && line.contains("governance=supported"))
    );
    assert!(lines.iter().any(|line| {
        line.contains("daemon lifecycle health") && line.contains("consolidation=healthy")
    }));
}

#[test]
fn doctor_reports_memory_rollout_missing_config() {
    let config = Config::default();
    let snapshot = serde_json::json!({});

    let lines = memory_rollout_lines(&config, &snapshot);

    assert!(
        lines
            .iter()
            .any(|line| line.contains("missing config") && line.contains("non-fatal"))
    );
    assert!(lines.iter().any(|line| line.contains("action:")));
}
