use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::sync::Arc;

const CONTRADICTION_RATIO_SLO_MAX: f64 = 0.20;

pub(super) fn run_memory_hygiene_tick(
    config: &crate::config::Config,
    observer: &Arc<dyn crate::runtime::observability::Observer>,
) {
    if !config.memory.hygiene_enabled {
        return;
    }

    match crate::core::memory::hygiene::run_if_due(&config.memory, &config.workspace_dir) {
        Ok(()) => {
            crate::runtime::diagnostics::health::mark_component_ok("memory_hygiene");
        }
        Err(error) => {
            crate::runtime::diagnostics::health::mark_component_error(
                "memory_hygiene",
                error.to_string(),
            );
            tracing::warn!(%error, "memory hygiene tick failed");
        }
    }

    if let Ok(Some(total)) = contradiction_mark_total(&config.workspace_dir) {
        observer.record_metric(
            &crate::runtime::observability::traits::ObserverMetric::ContradictionMarkTotal {
                count: total,
            },
        );
        tracing::info!(
            contradiction_mark_total = total,
            "memory contradiction metric snapshot"
        );
    }

    if let Ok(Some(total)) = belief_promotion_total(&config.workspace_dir) {
        observer.record_metric(
            &crate::runtime::observability::traits::ObserverMetric::BeliefPromotionTotal {
                count: total,
            },
        );
        tracing::info!(
            belief_promotion_total = total,
            "memory promotion metric snapshot"
        );
    }

    if let Ok(Some(total)) = stale_trend_purge_total(&config.workspace_dir) {
        observer.record_metric(
            &crate::runtime::observability::traits::ObserverMetric::StaleTrendPurgeTotal {
                count: total,
            },
        );
        tracing::info!(
            stale_trend_purge_total = total,
            "memory stale trend metric snapshot"
        );
    }

    record_signal_distribution_snapshot(&config.workspace_dir, observer);

    evaluate_memory_slo(&config.workspace_dir, observer);
}

fn record_signal_distribution_snapshot(
    workspace_dir: &Path,
    observer: &Arc<dyn crate::runtime::observability::Observer>,
) {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return;
    }

    let Ok(conn) = Connection::open(db_path) else {
        return;
    };

    if let Ok(mut stmt) =
        conn.prepare("SELECT signal_tier, COUNT(*) FROM retrieval_units GROUP BY signal_tier")
        && let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
    {
        for (tier, count) in rows.flatten() {
            observer.record_metric(
                &crate::runtime::observability::traits::ObserverMetric::SignalTierSnapshot {
                    tier,
                    count: u64::try_from(count).unwrap_or(0),
                },
            );
        }
    }

    if let Ok(mut stmt) = conn
        .prepare("SELECT promotion_status, COUNT(*) FROM retrieval_units GROUP BY promotion_status")
        && let Ok(rows) = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
    {
        for (status, count) in rows.flatten() {
            observer.record_metric(
                &crate::runtime::observability::traits::ObserverMetric::PromotionStatusSnapshot {
                    status,
                    count: u64::try_from(count).unwrap_or(0),
                },
            );
        }
    }
}

pub(super) fn evaluate_memory_slo(
    workspace_dir: &Path,
    observer: &Arc<dyn crate::runtime::observability::Observer>,
) {
    let Ok(Some(ratio)) = contradiction_ratio(workspace_dir) else {
        return;
    };

    if ratio > CONTRADICTION_RATIO_SLO_MAX {
        let message = format!(
            "contradiction_ratio_slo_violation ratio={ratio:.3} threshold={CONTRADICTION_RATIO_SLO_MAX:.3}"
        );
        observer.record_metric(
            &crate::runtime::observability::traits::ObserverMetric::MemorySloViolation,
        );
        crate::runtime::diagnostics::health::mark_component_error("memory_slo", message.clone());
        tracing::warn!(contradiction_ratio = ratio, "{message}");
    } else {
        crate::runtime::diagnostics::health::mark_component_ok("memory_slo");
    }
}

pub(super) fn contradiction_mark_total(workspace_dir: &Path) -> Result<Option<u64>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let conn = Connection::open(db_path)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_events WHERE event_type = ?1",
        params!["contradiction_marked"],
        |row| row.get(0),
    )?;
    Ok(Some(u64::try_from(count).unwrap_or(0)))
}

pub(super) fn contradiction_ratio(workspace_dir: &Path) -> Result<Option<f64>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let conn = Connection::open(db_path)?;
    let total: i64 =
        conn.query_row("SELECT COUNT(*) FROM retrieval_units", [], |row| row.get(0))?;
    if total <= 0 {
        return Ok(Some(0.0));
    }

    let contradicted: i64 = conn.query_row(
        "SELECT COUNT(*) FROM retrieval_units WHERE contradiction_penalty > ?1",
        params![0.0_f64],
        |row| row.get(0),
    )?;

    let total_u32 = u32::try_from(total).unwrap_or(u32::MAX);
    let contradicted_u32 = u32::try_from(contradicted).unwrap_or(u32::MAX);

    Ok(Some(
        f64::from(contradicted_u32) / f64::from(total_u32.max(1)),
    ))
}

pub(super) fn belief_promotion_total(workspace_dir: &Path) -> Result<Option<u64>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let conn = Connection::open(db_path)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM retrieval_units
         WHERE promotion_status IN ('candidate', 'promoted')",
        [],
        |row| row.get(0),
    )?;
    Ok(Some(u64::try_from(count).unwrap_or(0)))
}

pub(super) fn stale_trend_purge_total(workspace_dir: &Path) -> Result<Option<u64>> {
    let state_path = workspace_dir
        .join("state")
        .join("memory_hygiene_state.json");
    if !state_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(state_path)?;
    let json: Value = serde_json::from_str(&raw)?;
    let total = json
        .get("last_report")
        .and_then(|v| v.get("lifecycle"))
        .and_then(|v| v.get("stale_trend_demoted"))
        .and_then(Value::as_u64);
    Ok(total)
}
