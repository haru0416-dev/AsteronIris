mod report;
mod setup;

#[cfg(test)]
mod tests;

use crate::config::Config;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use report::{autonomy_governance_lines, memory_rollout_lines, parse_rfc3339};
use setup::run_setup_checks;

const DAEMON_STALE_SECONDS: i64 = 30;
const SCHEDULER_STALE_SECONDS: i64 = 120;
const CHANNEL_STALE_SECONDS: i64 = 300;

pub fn run(config: &Config) -> Result<()> {
    println!("◆ {}", t!("doctor.title"));
    println!();

    print_setup_health(config);
    print_daemon_health(config)?;
    print_governance_and_rollout(config);

    Ok(())
}

// ── Setup Health ──

fn print_setup_health(config: &Config) {
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
}

// ── Daemon Health ──

fn print_daemon_health(config: &Config) -> Result<()> {
    println!("  Daemon Health");
    println!("  {}", "─".repeat(50));
    let state_file = crate::platform::daemon::state_file_path(config);
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

        let (channel_count, stale_channels) = check_daemon_components(&snapshot);

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
    Ok(())
}

fn check_daemon_components(snapshot: &serde_json::Value) -> (u32, u32) {
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

    (channel_count, stale_channels)
}

fn print_governance_and_rollout(config: &Config) {
    println!("  {}", t!("doctor.autonomy_governance"));
    for line in autonomy_governance_lines(config) {
        println!("    {line}");
    }

    println!("  {}", t!("doctor.memory_rollout"));
    let state_file = crate::platform::daemon::state_file_path(config);
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
}
