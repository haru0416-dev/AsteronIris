use super::heartbeat_temperature;
use super::record_autonomy_mode_transition;
use super::run_memory_hygiene_tick;
use super::{
    belief_promotion_total, contradiction_mark_total, contradiction_ratio, evaluate_memory_slo,
    stale_trend_purge_total,
};
use crate::config::Config;
use crate::memory::SqliteMemory;
use crate::memory::traits::Memory;
use crate::memory::types::{MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel};
use crate::security::AutonomyLevel;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn memory_hygiene_tick_succeeds_with_fresh_workspace() {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();
    config.memory.hygiene_enabled = true;

    run_memory_hygiene_tick(&config);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn contradiction_ratio_reports_non_zero_when_penalty_exists() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());

    let _: crate::memory::types::MemoryEvent = mem
        .append_event(MemoryEventInput::new(
            "entity:test",
            "profile.name",
            MemoryEventType::FactAdded,
            "Alice",
            MemorySource::ExplicitUser,
            PrivacyLevel::Private,
        ))
        .await
        .unwrap();

    let _: crate::memory::types::MemoryEvent = mem
        .append_event(MemoryEventInput::new(
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn contradiction_ratio_returns_zero_when_units_absent() {
    let tmp = TempDir::new().unwrap();
    let _memory = SqliteMemory::new(tmp.path()).await.unwrap();

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
fn record_autonomy_mode_transition_does_not_crash_on_fresh_workspace() {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();
    config.autonomy.level = AutonomyLevel::ReadOnly;

    record_autonomy_mode_transition(&config);

    config.autonomy.level = AutonomyLevel::Full;
    record_autonomy_mode_transition(&config);
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

    record_autonomy_mode_transition(&config);

    config.autonomy.level = AutonomyLevel::ReadOnly;
    record_autonomy_mode_transition(&config);
}

#[test]
fn evaluate_memory_slo_with_no_data_emits_no_crash() {
    let tmp = TempDir::new().unwrap();
    evaluate_memory_slo(tmp.path());
}
