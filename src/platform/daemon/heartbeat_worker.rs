use crate::runtime::observability::traits::AutonomyLifecycleSignal;
use anyhow::Result;
use rusqlite::{Connection, params};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::time::Duration;

const CONTRADICTION_RATIO_SLO_MAX: f64 = 0.20;

fn run_memory_hygiene_tick(
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

fn evaluate_memory_slo(
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

fn heartbeat_temperature(config: &crate::config::Config) -> f64 {
    config
        .autonomy
        .clamp_temperature(config.default_temperature)
}

fn contradiction_mark_total(workspace_dir: &Path) -> Result<Option<u64>> {
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

fn contradiction_ratio(workspace_dir: &Path) -> Result<Option<f64>> {
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

fn belief_promotion_total(workspace_dir: &Path) -> Result<Option<u64>> {
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

fn stale_trend_purge_total(workspace_dir: &Path) -> Result<Option<u64>> {
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

fn autonomy_level_to_str(level: crate::security::AutonomyLevel) -> &'static str {
    match level {
        crate::security::AutonomyLevel::ReadOnly => "read_only",
        crate::security::AutonomyLevel::Supervised => "supervised",
        crate::security::AutonomyLevel::Full => "full",
    }
}

fn read_last_autonomy_level(workspace_dir: &Path) -> Option<String> {
    let path = workspace_dir.join("state").join("autonomy_mode_state.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&raw)
        .ok()?
        .get("last")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn write_last_autonomy_level(workspace_dir: &Path, level: &str) -> Result<()> {
    let state_dir = workspace_dir.join("state");
    fs::create_dir_all(&state_dir)?;
    let path = state_dir.join("autonomy_mode_state.json");
    let payload = serde_json::json!({"last": level});
    fs::write(path, serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

fn record_autonomy_mode_transition(
    config: &crate::config::Config,
    observer: &Arc<dyn crate::runtime::observability::Observer>,
) {
    let current = autonomy_level_to_str(config.autonomy.effective_autonomy_level()).to_string();
    if let Some(previous) = read_last_autonomy_level(&config.workspace_dir)
        && previous != current
    {
        observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::ModeTransition);
        tracing::info!(from = previous, to = current, "autonomy mode transitioned");
    }
    let _ = write_last_autonomy_level(&config.workspace_dir, &current);
}

pub(super) async fn run_heartbeat_worker(config: Arc<crate::config::Config>) -> Result<()> {
    let observer: Arc<dyn crate::runtime::observability::Observer> = Arc::from(
        crate::runtime::observability::create_observer(&config.observability),
    );
    let engine = crate::runtime::diagnostics::heartbeat::engine::HeartbeatEngine::new(
        config.heartbeat.clone(),
        config.workspace_dir.clone(),
        Arc::clone(&observer),
    );

    let interval_mins = config.heartbeat.interval_minutes.max(5);
    let mut interval = tokio::time::interval(Duration::from_secs(u64::from(interval_mins) * 60));

    loop {
        interval.tick().await;
        run_memory_hygiene_tick(&config, &observer);
        record_autonomy_mode_transition(&config, &observer);

        let tasks = engine.collect_tasks().await?;
        if tasks.is_empty() {
            continue;
        }

        for task in tasks {
            let prompt = format!("[Heartbeat Task] {task}");
            let temp = heartbeat_temperature(&config);
            if let Err(e) =
                crate::core::agent::run(Arc::clone(&config), Some(prompt), None, None, temp).await
            {
                crate::runtime::diagnostics::health::mark_component_error(
                    "heartbeat",
                    e.to_string(),
                );
                tracing::warn!("Heartbeat task failed: {e}");
            } else {
                crate::runtime::diagnostics::health::mark_component_ok("heartbeat");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::heartbeat_temperature;
    use super::record_autonomy_mode_transition;
    use super::run_memory_hygiene_tick;
    use super::{
        belief_promotion_total, contradiction_mark_total, contradiction_ratio, evaluate_memory_slo,
        stale_trend_purge_total,
    };
    use crate::config::Config;
    use crate::core::memory::traits::MemoryLayer;
    use crate::core::memory::{
        Memory, MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SqliteMemory,
    };
    use crate::runtime::observability::PrometheusObserver;
    use crate::security::AutonomyLevel;
    use rusqlite::{Connection, params};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn memory_hygiene_tick_succeeds_with_fresh_workspace() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.memory.hygiene_enabled = true;
        let observer = Arc::new(PrometheusObserver::new());

        run_memory_hygiene_tick(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let signal = observer.snapshot_signal_counts();
        assert!(signal.tier_snapshot.is_empty());
        assert!(signal.promotion_status_snapshot.is_empty());

        let snapshot = crate::runtime::diagnostics::health::snapshot_json();
        let status = snapshot["components"]["memory_hygiene"]["status"]
            .as_str()
            .unwrap_or("");
        assert_eq!(status, "ok");
    }

    #[tokio::test]
    async fn memory_hygiene_tick_records_signal_distribution_metrics() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.memory.hygiene_enabled = true;

        let memory = SqliteMemory::new(tmp.path()).unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "entity:test",
                    "metric.signal.raw",
                    MemoryEventType::FactAdded,
                    "raw signal",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        drop(memory);

        let db_path = tmp.path().join("memory").join("brain.db");
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "UPDATE retrieval_units SET signal_tier = 'raw', promotion_status = 'promoted' WHERE unit_id = ?1",
            params!["entity:test:metric.signal.raw"],
        )
        .unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        run_memory_hygiene_tick(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let signal = observer.snapshot_signal_counts();
        assert_eq!(signal.tier_snapshot.get("raw"), Some(&1));
        assert_eq!(signal.promotion_status_snapshot.get("promoted"), Some(&1));
    }

    #[tokio::test]
    async fn memory_hygiene_tick_records_multi_tier_and_status_distribution() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.memory.hygiene_enabled = true;

        let memory = SqliteMemory::new(tmp.path()).unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "entity:test",
                    "metric.signal.raw.1",
                    MemoryEventType::FactAdded,
                    "raw signal one",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "entity:test",
                    "metric.signal.raw.2",
                    MemoryEventType::FactAdded,
                    "raw signal two",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "entity:test",
                    "metric.signal.candidate.1",
                    MemoryEventType::FactAdded,
                    "candidate signal",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        drop(memory);

        let db_path = tmp.path().join("memory").join("brain.db");
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "UPDATE retrieval_units SET signal_tier = 'raw', promotion_status = 'demoted' WHERE unit_id = ?1",
            params!["entity:test:metric.signal.raw.1"],
        )
        .unwrap();
        conn.execute(
            "UPDATE retrieval_units SET signal_tier = 'raw', promotion_status = 'promoted' WHERE unit_id = ?1",
            params!["entity:test:metric.signal.raw.2"],
        )
        .unwrap();
        conn.execute(
            "UPDATE retrieval_units SET signal_tier = 'candidate', promotion_status = 'candidate' WHERE unit_id = ?1",
            params!["entity:test:metric.signal.candidate.1"],
        )
        .unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        run_memory_hygiene_tick(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let signal = observer.snapshot_signal_counts();
        assert_eq!(signal.tier_snapshot.get("raw"), Some(&2));
        assert_eq!(signal.tier_snapshot.get("candidate"), Some(&1));
        assert_eq!(signal.promotion_status_snapshot.get("demoted"), Some(&1));
        assert_eq!(signal.promotion_status_snapshot.get("promoted"), Some(&1));
        assert_eq!(signal.promotion_status_snapshot.get("candidate"), Some(&1));
    }

    #[tokio::test]
    async fn contradiction_ratio_reports_non_zero_when_penalty_exists() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());

        mem.append_event(MemoryEventInput::new(
            "entity:test",
            "profile.name",
            MemoryEventType::FactAdded,
            "Alice",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        ))
        .await
        .unwrap();

        mem.append_event(MemoryEventInput::new(
            "entity:test",
            "profile.name",
            MemoryEventType::ContradictionMarked,
            "Name conflict",
            MemorySource::System,
            PrivacyLevel::Private,
        ))
        .await
        .unwrap();

        let ratio = contradiction_ratio(tmp.path()).unwrap().unwrap();
        assert!(ratio > 0.0);

        let total = contradiction_mark_total(tmp.path()).unwrap().unwrap();
        assert!(total >= 1);

        let belief_total = belief_promotion_total(tmp.path()).unwrap().unwrap();
        assert!(belief_total >= 1);
    }

    #[tokio::test]
    async fn contradiction_ratio_returns_zero_when_units_absent() {
        let tmp = TempDir::new().unwrap();
        let _memory = SqliteMemory::new(tmp.path()).unwrap();

        let ratio = contradiction_ratio(tmp.path()).unwrap().unwrap();
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn stale_trend_purge_total_reads_hygiene_state_file() {
        let tmp = TempDir::new().unwrap();
        let state_dir = tmp.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("memory_hygiene_state.json"),
            r#"{
  "last_run_at": "2026-02-23T00:00:00Z",
  "last_report": {
    "lifecycle": {
      "stale_trend_demoted": 7
    }
  }
}"#,
        )
        .unwrap();

        let total = stale_trend_purge_total(tmp.path()).unwrap().unwrap();
        assert_eq!(total, 7);
    }

    #[test]
    fn stale_trend_purge_total_returns_none_when_state_missing() {
        let tmp = TempDir::new().unwrap();
        let total = stale_trend_purge_total(tmp.path()).unwrap();
        assert!(total.is_none());
    }

    #[test]
    fn memory_hygiene_tick_ignores_malformed_hygiene_state_snapshot() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.memory.hygiene_enabled = true;

        let state_dir = tmp.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("memory_hygiene_state.json"),
            "{ this is not valid json",
        )
        .unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        run_memory_hygiene_tick(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let snapshot = crate::runtime::diagnostics::health::snapshot_json();
        let status = snapshot["components"]["memory_hygiene"]["status"]
            .as_str()
            .unwrap_or("");
        assert_eq!(status, "ok");
    }

    #[tokio::test]
    async fn memory_hygiene_tick_records_promotion_observability_snapshots() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.memory.hygiene_enabled = true;

        let memory = SqliteMemory::new(tmp.path()).unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "entity:obs",
                    "obs.slot",
                    MemoryEventType::FactAdded,
                    "observability payload",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "entity:obs",
                    "obs.slot",
                    MemoryEventType::ContradictionMarked,
                    "conflict",
                    MemorySource::System,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        drop(memory);

        let db_path = tmp.path().join("memory").join("brain.db");
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "UPDATE retrieval_units SET promotion_status = 'candidate' WHERE unit_id = ?1",
            params!["entity:obs:obs.slot"],
        )
        .unwrap();
        drop(conn);

        let state_dir = tmp.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(
            state_dir.join("memory_hygiene_state.json"),
            format!(
                "{{\n  \"last_run_at\": \"{}\",\n  \"last_report\": {{\n    \"archived_memory_files\": 0,\n    \"archived_session_files\": 0,\n    \"purged_memory_archives\": 0,\n    \"purged_session_archives\": 0,\n    \"pruned_conversation_rows\": 0,\n    \"lifecycle\": {{\n      \"ttl_slot_hard_deleted\": 0,\n      \"ttl_unit_purged\": 0,\n      \"low_confidence_demoted\": 0,\n      \"contradiction_auto_demoted\": 0,\n      \"stale_trend_demoted\": 9,\n      \"recency_refreshed\": 0,\n      \"layer_cleanup_actions\": 0,\n      \"ledger_purged\": 0\n    }}\n  }}\n}}",
                chrono::Utc::now().to_rfc3339()
            ),
        )
        .unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        run_memory_hygiene_tick(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let signal = observer.snapshot_signal_counts();
        assert_eq!(signal.belief_promotion_total_snapshot, 1);
        assert_eq!(signal.contradiction_mark_total_snapshot, 1);
        assert_eq!(signal.stale_trend_purge_total_snapshot, 9);
    }

    #[test]
    fn contradiction_and_belief_totals_return_none_when_db_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(contradiction_mark_total(tmp.path()).unwrap().is_none());
        assert!(belief_promotion_total(tmp.path()).unwrap().is_none());
        assert!(contradiction_ratio(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn heartbeat_temperature_clamps_default_temperature() {
        let mut config = Config::default();
        config.default_temperature = 1.4;
        config.autonomy.level = AutonomyLevel::Supervised;
        config.autonomy.temperature_bands.supervised.min = 0.2;
        config.autonomy.temperature_bands.supervised.max = 0.7;

        assert!((heartbeat_temperature(&config) - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn heartbeat_temperature_clamps_to_lower_bound() {
        let mut config = Config::default();
        config.default_temperature = 0.05;
        config.autonomy.level = AutonomyLevel::Supervised;
        config.autonomy.temperature_bands.supervised.min = 0.2;
        config.autonomy.temperature_bands.supervised.max = 0.7;

        assert!((heartbeat_temperature(&config) - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn heartbeat_temperature_keeps_value_within_band() {
        let mut config = Config::default();
        config.default_temperature = 0.45;
        config.autonomy.level = AutonomyLevel::Supervised;
        config.autonomy.temperature_bands.supervised.min = 0.2;
        config.autonomy.temperature_bands.supervised.max = 0.7;

        assert!((heartbeat_temperature(&config) - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn heartbeat_temperature_clamps_for_read_only_band() {
        let mut config = Config::default();
        config.default_temperature = 0.95;
        config.autonomy.level = AutonomyLevel::ReadOnly;
        config.autonomy.temperature_bands.read_only.min = 0.1;
        config.autonomy.temperature_bands.read_only.max = 0.3;

        assert!((heartbeat_temperature(&config) - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn heartbeat_temperature_clamps_for_full_band_lower_bound() {
        let mut config = Config::default();
        config.default_temperature = 0.01;
        config.autonomy.level = AutonomyLevel::Full;
        config.autonomy.temperature_bands.full.min = 0.2;
        config.autonomy.temperature_bands.full.max = 0.9;

        assert!((heartbeat_temperature(&config) - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn record_autonomy_mode_transition_emits_metric_on_change() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.autonomy.level = AutonomyLevel::ReadOnly;

        let observer = Arc::new(PrometheusObserver::new());
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        config.autonomy.level = AutonomyLevel::Full;
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let counts = observer.snapshot_autonomy_counts();
        assert_eq!(counts.mode_transition, 1);
    }

    #[test]
    fn record_autonomy_mode_transition_does_not_emit_when_unchanged() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.autonomy.level = AutonomyLevel::Supervised;

        let observer = Arc::new(PrometheusObserver::new());
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let counts = observer.snapshot_autonomy_counts();
        assert_eq!(counts.mode_transition, 0);
    }

    #[test]
    fn record_autonomy_mode_transition_emits_once_per_actual_change() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();

        let observer = Arc::new(PrometheusObserver::new());

        config.autonomy.level = AutonomyLevel::Supervised;
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        config.autonomy.level = AutonomyLevel::Full;
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        config.autonomy.level = AutonomyLevel::ReadOnly;
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let counts = observer.snapshot_autonomy_counts();
        assert_eq!(counts.mode_transition, 2);
    }

    #[test]
    fn record_autonomy_mode_transition_recovers_from_malformed_state_file() {
        let tmp = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = tmp.path().to_path_buf();
        config.autonomy.level = AutonomyLevel::Full;

        let state_dir = tmp.path().join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        std::fs::write(state_dir.join("autonomy_mode_state.json"), "{ malformed").unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let counts = observer.snapshot_autonomy_counts();
        assert_eq!(counts.mode_transition, 0);

        config.autonomy.level = AutonomyLevel::ReadOnly;
        record_autonomy_mode_transition(
            &config,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let counts = observer.snapshot_autonomy_counts();
        assert_eq!(counts.mode_transition, 1);
    }

    #[tokio::test]
    async fn evaluate_memory_slo_marks_violation_on_high_contradiction_ratio() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path();
        let memory = SqliteMemory::new(workspace).unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "default",
                    "slo.slot.1",
                    MemoryEventType::FactAdded,
                    "slo sample",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();
        drop(memory);

        let db_path = workspace.join("memory").join("brain.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE retrieval_units SET contradiction_penalty = 1.0 WHERE unit_id = ?1",
            params!["default:slo.slot.1"],
        )
        .unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        evaluate_memory_slo(
            workspace,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );
        let snapshot = crate::runtime::diagnostics::health::snapshot();
        let status = snapshot.components.get("memory_slo").unwrap();
        assert_eq!(status.status, "error");
        assert!(
            status
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("contradiction_ratio_slo_violation")
        );
        assert_eq!(observer.snapshot_memory_slo_violation_count(), 1);
    }

    #[tokio::test]
    async fn evaluate_memory_slo_marks_ok_below_threshold() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path();
        let memory = SqliteMemory::new(workspace).unwrap();
        memory
            .append_event(
                MemoryEventInput::new(
                    "default",
                    "slo.slot.2",
                    MemoryEventType::FactAdded,
                    "slo sample low contradiction",
                    MemorySource::ExplicitUser,
                    PrivacyLevel::Private,
                )
                .with_layer(MemoryLayer::Working),
            )
            .await
            .unwrap();

        let observer = Arc::new(PrometheusObserver::new());
        evaluate_memory_slo(
            workspace,
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );
        let snapshot = crate::runtime::diagnostics::health::snapshot();
        let status = snapshot.components.get("memory_slo").unwrap();
        assert_eq!(status.status, "ok");
        assert_eq!(observer.snapshot_memory_slo_violation_count(), 0);
    }

    #[tokio::test]
    async fn evaluate_memory_slo_keeps_ok_at_threshold_boundary() {
        let tmp = TempDir::new().unwrap();
        let memory = SqliteMemory::new(tmp.path()).unwrap();

        memory
            .append_event(MemoryEventInput::new(
                "default",
                "slo-threshold-1",
                MemoryEventType::FactAdded,
                "v1",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            ))
            .await
            .unwrap();
        memory
            .append_event(MemoryEventInput::new(
                "default",
                "slo-threshold-2",
                MemoryEventType::FactAdded,
                "v2",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            ))
            .await
            .unwrap();
        memory
            .append_event(MemoryEventInput::new(
                "default",
                "slo-threshold-3",
                MemoryEventType::FactAdded,
                "v3",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            ))
            .await
            .unwrap();
        memory
            .append_event(MemoryEventInput::new(
                "default",
                "slo-threshold-4",
                MemoryEventType::FactAdded,
                "v4",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            ))
            .await
            .unwrap();
        memory
            .append_event(MemoryEventInput::new(
                "default",
                "slo-threshold-5",
                MemoryEventType::FactAdded,
                "v5",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            ))
            .await
            .unwrap();

        let conn = Connection::open(tmp.path().join("memory").join("brain.db")).unwrap();
        conn.execute(
            "UPDATE retrieval_units SET contradiction_penalty = 1.0 WHERE slot_key = 'slo-threshold-5'",
            [],
        )
        .unwrap();
        drop(conn);

        crate::runtime::diagnostics::health::mark_component_ok("memory_slo");

        let observer = Arc::new(PrometheusObserver::new());
        evaluate_memory_slo(
            tmp.path(),
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        let snapshot = crate::runtime::diagnostics::health::snapshot();
        let status = snapshot.components.get("memory_slo").unwrap();
        assert_eq!(status.status, "ok");
        assert_eq!(observer.snapshot_memory_slo_violation_count(), 0);
    }

    #[test]
    fn evaluate_memory_slo_with_no_data_emits_no_violation_metric() {
        let tmp = TempDir::new().unwrap();
        let observer = Arc::new(PrometheusObserver::new());

        evaluate_memory_slo(
            tmp.path(),
            &(observer.clone() as Arc<dyn crate::runtime::observability::Observer>),
        );

        assert_eq!(observer.snapshot_memory_slo_violation_count(), 0);
    }
}
