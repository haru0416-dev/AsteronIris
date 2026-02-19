use crate::config::Config;
use crate::memory::CapabilitySupport;
use crate::security::ExternalActionExecution;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

const DAEMON_STALE_SECONDS: i64 = 30;
const SCHEDULER_STALE_SECONDS: i64 = 120;
const CHANNEL_STALE_SECONDS: i64 = 300;

#[allow(clippy::too_many_lines)]
pub fn run(config: &Config) -> Result<()> {
    let state_file = crate::daemon::state_file_path(config);
    if !state_file.exists() {
        println!("🩺 AsteronIris Doctor");
        println!("  ❌ daemon state file not found: {}", state_file.display());
        println!("  💡 Start daemon with: asteroniris daemon");
        return Ok(());
    }

    let raw = std::fs::read_to_string(&state_file)
        .with_context(|| format!("Failed to read {}", state_file.display()))?;
    let snapshot: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", state_file.display()))?;

    println!("🩺 AsteronIris Doctor");
    println!("  State file: {}", state_file.display());

    let updated_at = snapshot
        .get("updated_at")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    if let Ok(ts) = DateTime::parse_from_rfc3339(updated_at) {
        let age = Utc::now()
            .signed_duration_since(ts.with_timezone(&Utc))
            .num_seconds();
        if age <= DAEMON_STALE_SECONDS {
            println!("  ✅ daemon heartbeat fresh ({age}s ago)");
        } else {
            println!("  ❌ daemon heartbeat stale ({age}s ago)");
        }
    } else {
        println!("  ❌ invalid daemon timestamp: {updated_at}");
    }

    let mut channel_count = 0_u32;
    let mut stale_channels = 0_u32;

    if let Some(components) = snapshot
        .get("components")
        .and_then(serde_json::Value::as_object)
    {
        if let Some(scheduler) = components.get("scheduler") {
            let scheduler_ok = scheduler
                .get("status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| s == "ok");

            let scheduler_last_ok = scheduler
                .get("last_ok")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_rfc3339)
                .map_or(i64::MAX, |dt| {
                    Utc::now().signed_duration_since(dt).num_seconds()
                });

            if scheduler_ok && scheduler_last_ok <= SCHEDULER_STALE_SECONDS {
                println!("  ✅ scheduler healthy (last ok {scheduler_last_ok}s ago)");
            } else {
                println!(
                    "  ❌ scheduler unhealthy/stale (status_ok={scheduler_ok}, age={scheduler_last_ok}s)"
                );
            }
        } else {
            println!("  ❌ scheduler component missing");
        }

        for (name, component) in components {
            if !name.starts_with("channel:") {
                continue;
            }

            channel_count += 1;
            let status_ok = component
                .get("status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| s == "ok");
            let age = component
                .get("last_ok")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_rfc3339)
                .map_or(i64::MAX, |dt| {
                    Utc::now().signed_duration_since(dt).num_seconds()
                });

            if status_ok && age <= CHANNEL_STALE_SECONDS {
                println!("  ✅ {name} fresh (last ok {age}s ago)");
            } else {
                stale_channels += 1;
                println!("  ❌ {name} stale/unhealthy (status_ok={status_ok}, age={age}s)");
            }
        }
    }

    if channel_count == 0 {
        println!("  ℹ️ no channel components tracked in state yet");
    } else {
        println!("  Channel summary: {channel_count} total, {stale_channels} stale");
    }

    println!("  Autonomy governance:");
    for line in autonomy_governance_lines(config) {
        println!("    {line}");
    }

    println!("  Memory rollout:");
    for line in memory_rollout_lines(config, &snapshot) {
        println!("    {line}");
    }

    if let Some(runtime_note) =
        crate::runtime::runtime_kind_contract_note(config.runtime.kind.as_str())
    {
        println!("  Runtime contract:");
        println!("    runtime.kind='{}': {runtime_note}", config.runtime.kind);
    }

    Ok(())
}

fn autonomy_governance_lines(config: &Config) -> Vec<String> {
    let mut lines = Vec::with_capacity(6);

    lines.push(format!("autonomy level: {:?}", config.autonomy.level));

    let external_actions = match config.autonomy.external_action_execution {
        ExternalActionExecution::Disabled => "disabled",
        ExternalActionExecution::Enabled => "enabled",
    };
    lines.push(format!("external actions: {external_actions}"));

    let selected_band = config.autonomy.selected_temperature_band();
    lines.push(format!(
        "temperature band: [{:.2}, {:.2}]",
        selected_band.min, selected_band.max
    ));

    let rollout_stage = match config.autonomy.rollout.stage {
        Some(crate::config::schema::AutonomyRolloutStage::ReadOnly) => "read-only",
        Some(crate::config::schema::AutonomyRolloutStage::Supervised) => "supervised",
        Some(crate::config::schema::AutonomyRolloutStage::Full) => "full",
        None => "off",
    };
    lines.push(format!("rollout stage: {rollout_stage}"));
    lines.push(format!(
        "rollout policy: enabled={}, read_only_days={:?}, supervised_days={:?}",
        if config.autonomy.rollout.enabled {
            "on"
        } else {
            "off"
        },
        config.autonomy.rollout.read_only_days,
        config.autonomy.rollout.supervised_days
    ));
    lines.push(format!(
        "verify/repair caps: max_attempts={}, max_repair_depth={}",
        config.autonomy.verify_repair_max_attempts, config.autonomy.verify_repair_max_repair_depth
    ));

    let backend = config.observability.backend.as_str();
    let lifecycle_metrics = if backend_supports_autonomy_lifecycle_metrics(backend) {
        "enabled"
    } else {
        "disabled"
    };
    lines.push(format!(
        "observability backend: {backend} (autonomy lifecycle metrics: {lifecycle_metrics})"
    ));

    lines
}

fn backend_supports_autonomy_lifecycle_metrics(backend: &str) -> bool {
    matches!(backend, "log" | "prometheus" | "otel")
}

fn memory_rollout_lines(config: &Config, snapshot: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::with_capacity(5);
    let backend = config.memory.backend.as_str();
    let capability = crate::memory::capability_matrix_for_backend(backend);

    let consolidation = if backend != "none" && config.memory.auto_save {
        "on"
    } else {
        "off"
    };
    let conflict = if backend != "none" && config.autonomy.rollout.enabled {
        "on"
    } else {
        "off"
    };
    let revocation = capability.map_or("unknown", |matrix| {
        capability_support_label(matrix.forget_tombstone)
    });
    let governance = capability.map_or("unknown", |matrix| {
        capability_support_label(matrix.forget_hard)
    });

    lines.push(format!(
        "memory backend: {backend} (consolidation={consolidation}, conflict={conflict})"
    ));
    lines.push(format!(
        "lifecycle support: revocation={revocation}, governance={governance}"
    ));

    if let Some(rollout) = snapshot
        .get("memory_rollout")
        .and_then(serde_json::Value::as_object)
    {
        let consolidation_health = rollout
            .get("consolidation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let conflict_health = rollout
            .get("conflict")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let revocation_health = rollout
            .get("revocation")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let governance_health = rollout
            .get("governance")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");

        lines.push(format!(
            "daemon lifecycle health: consolidation={consolidation_health}, conflict={conflict_health}, revocation={revocation_health}, governance={governance_health}"
        ));
    } else {
        lines.push(
            "daemon lifecycle health: missing config in state file; non-fatal, using static capability fallback".to_string(),
        );
        lines.push(
            "action: restart daemon after rollout update to surface consolidation/conflict/revocation/governance telemetry"
                .to_string(),
        );
    }

    lines
}

fn capability_support_label(support: CapabilitySupport) -> &'static str {
    match support {
        CapabilitySupport::Supported => "supported",
        CapabilitySupport::Degraded => "degraded",
        CapabilitySupport::Unsupported => "unsupported",
    }
}

fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::{autonomy_governance_lines, memory_rollout_lines};
    use crate::config::Config;

    #[test]
    fn doctor_reports_autonomy_gates() {
        let mut config = Config::default();
        config.observability.backend = "prometheus".into();

        let lines = autonomy_governance_lines(&config);

        assert!(lines
            .iter()
            .any(|line| line.contains("external actions") && line.contains("disabled")));
        assert!(lines
            .iter()
            .any(|line| line.contains("temperature band") && line.contains('[')));
        assert!(lines
            .iter()
            .any(|line| line.contains("rollout stage") && line.contains("off")));
        assert!(lines
            .iter()
            .any(|line| line.contains("rollout policy") && line.contains("enabled=off")));
        assert!(lines
            .iter()
            .any(|line| line.contains("observability backend") && line.contains("prometheus")));
        assert!(lines.iter().any(|line| {
            line.contains("autonomy lifecycle metrics") && line.contains("enabled")
        }));
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
        assert!(lines
            .iter()
            .any(|line| line.contains("revocation=supported")
                && line.contains("governance=supported")));
        assert!(lines.iter().any(|line| {
            line.contains("daemon lifecycle health") && line.contains("consolidation=healthy")
        }));
    }

    #[test]
    fn doctor_reports_memory_rollout_missing_config() {
        let config = Config::default();
        let snapshot = serde_json::json!({});

        let lines = memory_rollout_lines(&config, &snapshot);

        assert!(lines
            .iter()
            .any(|line| line.contains("missing config") && line.contains("non-fatal")));
        assert!(lines.iter().any(|line| line.contains("action:")));
    }
}
