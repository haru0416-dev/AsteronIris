use crate::config::Config;
use crate::security::ExternalActionExecution;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

const DAEMON_STALE_SECONDS: i64 = 30;
const SCHEDULER_STALE_SECONDS: i64 = 120;
const CHANNEL_STALE_SECONDS: i64 = 300;

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
        crate::config::schema::AutonomyRolloutStage::Off => "off",
        crate::config::schema::AutonomyRolloutStage::AuditOnly => "audit-only",
        crate::config::schema::AutonomyRolloutStage::Sanitize => "sanitize",
    };
    lines.push(format!("rollout stage: {rollout_stage}"));
    lines.push(format!(
        "rollout gates: verify_repair={}, contradiction_weighting={}, intent_audit_anomaly_detection={}",
        if config.autonomy.rollout.verify_repair_enabled {
            "on"
        } else {
            "off"
        },
        if config.autonomy.rollout.contradiction_weighting_enabled {
            "on"
        } else {
            "off"
        },
        if config
            .autonomy
            .rollout
            .intent_audit_anomaly_detection_enabled
        {
            "on"
        } else {
            "off"
        }
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

fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::autonomy_governance_lines;
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
            .any(|line| line.contains("temperature band") && line.contains("[")));
        assert!(lines
            .iter()
            .any(|line| line.contains("rollout stage") && line.contains("off")));
        assert!(lines
            .iter()
            .any(|line| line.contains("rollout gates") && line.contains("verify_repair=off")));
        assert!(lines
            .iter()
            .any(|line| line.contains("observability backend") && line.contains("prometheus")));
        assert!(lines.iter().any(|line| {
            line.contains("autonomy lifecycle metrics") && line.contains("enabled")
        }));
    }
}
