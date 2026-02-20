use crate::config::Config;
use crate::memory::CapabilitySupport;
use crate::security::ExternalActionExecution;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::UserDirs;

const DAEMON_STALE_SECONDS: i64 = 30;
const SCHEDULER_STALE_SECONDS: i64 = 120;
const CHANNEL_STALE_SECONDS: i64 = 300;

#[allow(clippy::too_many_lines)]
pub fn run(config: &Config) -> Result<()> {
    println!("◆ {}", t!("doctor.title"));
    println!();

    // ── Setup Health ──
    println!("  Setup Health");
    println!("  {}", "─".repeat(50));
    let setup_checks = run_setup_checks(config);
    let mut setup_warnings = 0u32;
    for (pass, msg) in &setup_checks {
        if *pass {
            println!("  ✓ {msg}");
        } else {
            setup_warnings += 1;
            println!("  ✗ {msg}");
        }
    }
    if setup_warnings == 0 {
        println!("  All setup checks passed.");
    } else {
        println!("  {setup_warnings} issue(s) found. Run 'asteroniris onboard' to fix.");
    }
    println!();

    // ── Daemon Health ──
    println!("  Daemon Health");
    println!("  {}", "─".repeat(50));
    let state_file = crate::daemon::state_file_path(config);
    if state_file.exists() {
        let raw = std::fs::read_to_string(&state_file)
            .with_context(|| format!("Failed to read {}", state_file.display()))?;
        let snapshot: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", state_file.display()))?;

        println!("  {}", t!("doctor.state_file", path = state_file.display()));

        let updated_at = snapshot
            .get("updated_at")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        if let Ok(ts) = DateTime::parse_from_rfc3339(updated_at) {
            let age = Utc::now()
                .signed_duration_since(ts.with_timezone(&Utc))
                .num_seconds();
            if age <= DAEMON_STALE_SECONDS {
                println!("  ✓ {}", t!("doctor.heartbeat_fresh", age = age));
            } else {
                println!("  ✗ {}", t!("doctor.heartbeat_stale", age = age));
            }
        } else {
            println!("  ✗ {}", t!("doctor.timestamp_invalid", value = updated_at));
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
                    println!(
                        "  ✓ {}",
                        t!("doctor.scheduler_healthy", age = scheduler_last_ok)
                    );
                } else {
                    println!(
                        "  ✗ {}",
                        t!(
                            "doctor.scheduler_unhealthy",
                            ok = scheduler_ok,
                            age = scheduler_last_ok
                        )
                    );
                }
            } else {
                println!("  ✗ {}", t!("doctor.scheduler_missing"));
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
                    println!("  ✓ {}", t!("doctor.channel_fresh", name = name, age = age));
                } else {
                    stale_channels += 1;
                    println!(
                        "  ✗ {}",
                        t!(
                            "doctor.channel_stale",
                            name = name,
                            ok = status_ok,
                            age = age
                        )
                    );
                }
            }
        }

        if channel_count == 0 {
            println!("  › {}", t!("doctor.no_channels"));
        } else {
            println!(
                "  {}",
                t!(
                    "doctor.channel_summary",
                    total = channel_count,
                    stale = stale_channels
                )
            );
        }
    } else {
        println!(
            "  ✗ {}",
            t!("doctor.state_not_found", path = state_file.display())
        );
        println!("  › {}", t!("doctor.start_hint"));
    }
    println!();

    println!("  {}", t!("doctor.autonomy_governance"));
    for line in autonomy_governance_lines(config) {
        println!("    {line}");
    }

    println!("  {}", t!("doctor.memory_rollout"));
    let state_file = crate::daemon::state_file_path(config);
    let snapshot = if state_file.exists() {
        std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
    } else {
        None
    };
    let snapshot = snapshot.unwrap_or_else(|| serde_json::json!({}));
    for line in memory_rollout_lines(config, &snapshot) {
        println!("    {line}");
    }

    Ok(())
}

fn run_setup_checks(config: &Config) -> Vec<(bool, String)> {
    let mut checks: Vec<(bool, String)> = Vec::new();

    let config_exists = config.config_path.exists();
    checks.push((
        config_exists,
        format!(
            "Config file: {}",
            if config_exists {
                config.config_path.display().to_string()
            } else {
                format!("missing ({})", config.config_path.display())
            }
        ),
    ));

    let ws_exists = config.workspace_dir.exists();
    checks.push((
        ws_exists,
        format!(
            "Workspace: {}",
            if ws_exists {
                config.workspace_dir.display().to_string()
            } else {
                format!("missing ({})", config.workspace_dir.display())
            }
        ),
    ));

    let has_provider = config.default_provider.is_some();
    checks.push((
        has_provider,
        format!(
            "Provider: {}",
            config
                .default_provider
                .as_deref()
                .unwrap_or("not configured — run: asteroniris onboard")
        ),
    ));

    let has_api_key = config.api_key.is_some()
        || std::env::var("ASTERONIRIS_API_KEY").is_ok()
        || std::env::var("API_KEY").is_ok();
    checks.push((
        has_api_key,
        if has_api_key {
            "API key: configured".to_string()
        } else {
            "API key: not set — run: asteroniris onboard".to_string()
        },
    ));

    let memory_ok = config.memory.backend != "none";
    checks.push((
        memory_ok,
        format!(
            "Memory: {} (auto-save: {})",
            config.memory.backend,
            if config.memory.auto_save { "on" } else { "off" }
        ),
    ));

    let service_installed = check_service_installed();
    checks.push((
        service_installed,
        if service_installed {
            "OS service: installed".to_string()
        } else {
            "OS service: not installed — optional, run: asteroniris service install".to_string()
        },
    ));

    checks
}

fn check_service_installed() -> bool {
    if cfg!(target_os = "macos") {
        UserDirs::new().is_some_and(|u| {
            u.home_dir()
                .join("Library")
                .join("LaunchAgents")
                .join("com.asteroniris.daemon.plist")
                .exists()
        })
    } else if cfg!(target_os = "linux") {
        UserDirs::new().is_some_and(|u| {
            u.home_dir()
                .join(".config")
                .join("systemd")
                .join("user")
                .join("asteroniris.service")
                .exists()
        })
    } else {
        false
    }
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
}
