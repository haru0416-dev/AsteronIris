use super::report::{autonomy_governance_lines, memory_rollout_lines, memory_signal_stats_lines};
use crate::config::Config;
use crate::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryLayer, MemorySource, PrivacyLevel,
    SqliteMemory,
};
use tempfile::TempDir;

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
        },
        "components": {
            "memory_slo": {
                "status": "ok"
            }
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
    assert!(
        lines
            .iter()
            .any(|line| line.contains("memory_slo component: ok"))
    );
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
    assert!(
        lines
            .iter()
            .any(|line| line.contains("memory_slo component: missing"))
    );
}

#[tokio::test]
async fn doctor_reports_memory_signal_stats() {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();

    let memory = SqliteMemory::new(tmp.path()).await.unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "doctor.signal.1",
                MemoryEventType::FactAdded,
                "doctor signal payload",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();

    let lines = memory_signal_stats_lines(&config).await;
    assert!(lines.iter().any(|line| line.contains("total_units=")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("promotion_breakdown") && line.contains("candidate="))
    );
    assert!(lines.iter().any(|line| line.contains("ttl_expired_units=")));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("source_kind_breakdown="))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("contradiction_ratio="))
    );
}

#[tokio::test]
async fn doctor_reports_memory_signal_ttl_and_promotion_breakdown() {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();

    let memory = SqliteMemory::new(tmp.path()).await.unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "doctor.signal.expired",
                MemoryEventType::FactAdded,
                "expired signal payload",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();

    // Use sqlx to update the test data directly
    let db_path = config.workspace_dir.join("memory").join("brain.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE retrieval_units SET promotion_status = 'promoted', source_kind = 'discord', retention_expires_at = datetime('now', '-1 day') WHERE slot_key = 'doctor.signal.expired'",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let lines = memory_signal_stats_lines(&config).await;
    assert!(lines.iter().any(|line| {
        line.contains("promotion_breakdown")
            && line.contains("promoted=1")
            && line.contains("demoted=0")
    }));
    assert!(
        lines
            .iter()
            .any(|line| line.contains("ttl_expired_units=1"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("source_kind_breakdown") && line.contains("discord=1"))
    );
}

#[tokio::test]
async fn doctor_source_kind_breakdown_is_sorted_stably() {
    let tmp = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = tmp.path().to_path_buf();

    let memory = SqliteMemory::new(tmp.path()).await.unwrap();
    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "doctor.signal.api",
                MemoryEventType::FactAdded,
                "api signal",
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
                "default",
                "doctor.signal.manual",
                MemoryEventType::FactAdded,
                "manual signal",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();

    let db_path = config.workspace_dir.join("memory").join("brain.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE retrieval_units SET source_kind = 'manual' WHERE slot_key = 'doctor.signal.manual'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE retrieval_units SET source_kind = 'api' WHERE slot_key = 'doctor.signal.api'",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let lines = memory_signal_stats_lines(&config).await;
    let breakdown = lines
        .iter()
        .find(|line| line.contains("source_kind_breakdown="))
        .expect("source_kind_breakdown line should exist");
    assert!(breakdown.contains("api=1,manual=1"));
}
