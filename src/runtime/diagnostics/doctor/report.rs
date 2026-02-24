use crate::config::Config;
use crate::memory::CapabilitySupport;
use crate::security::ExternalActionExecution;
use chrono::{DateTime, Utc};
use sqlx::Row;

pub(crate) fn autonomy_governance_lines(config: &Config) -> Vec<String> {
    let mut lines = Vec::with_capacity(6);

    lines.push(format!(
        "autonomy level: {:?}",
        config.autonomy.effective_autonomy_level()
    ));

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

pub(crate) fn memory_rollout_lines(config: &Config, snapshot: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::with_capacity(6);
    let backend = config.memory.backend.as_str();
    let capability = crate::memory::capability::capability_matrix_for_backend(backend);

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

    if let Some(memory_slo_status) = snapshot
        .get("components")
        .and_then(serde_json::Value::as_object)
        .and_then(|components| components.get("memory_slo"))
        .and_then(serde_json::Value::as_object)
        .and_then(|status| status.get("status"))
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("memory_slo component: {memory_slo_status}"));
    } else {
        lines.push("memory_slo component: missing".to_string());
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

pub(crate) fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Collect memory signal statistics from the `SQLite` brain database.
pub(crate) async fn memory_signal_stats_lines(config: &Config) -> Vec<String> {
    let db_path = config.workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return vec!["signal stats: memory db not found".to_string()];
    }

    let url = format!("sqlite://{}?mode=ro", db_path.display());
    let pool = match sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
    {
        Ok(pool) => pool,
        Err(error) => {
            return vec![format!("signal stats: failed to open memory db ({error})")];
        }
    };

    let count_query = |q: &'static str| {
        let pool = &pool;
        async move {
            sqlx::query_scalar::<_, i64>(q)
                .fetch_one(pool)
                .await
                .unwrap_or(0)
        }
    };

    let total_units = count_query("SELECT COUNT(*) FROM retrieval_units").await;
    let raw_units =
        count_query("SELECT COUNT(*) FROM retrieval_units WHERE signal_tier = 'raw'").await;
    let demoted_units =
        count_query("SELECT COUNT(*) FROM retrieval_units WHERE promotion_status = 'demoted'")
            .await;
    let candidate_units =
        count_query("SELECT COUNT(*) FROM retrieval_units WHERE promotion_status = 'candidate'")
            .await;
    let promoted_units =
        count_query("SELECT COUNT(*) FROM retrieval_units WHERE promotion_status = 'promoted'")
            .await;
    let ttl_expired_units = count_query(
        "SELECT COUNT(*) FROM retrieval_units WHERE retention_expires_at IS NOT NULL AND julianday(retention_expires_at) <= julianday('now')"
    ).await;
    let contradicted_units: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM retrieval_units WHERE contradiction_penalty > ?")
            .bind(0.0_f64)
            .fetch_one(&pool)
            .await
            .unwrap_or(0);

    let source_kind_breakdown = sqlx::query(
        "SELECT source_kind, COUNT(*) as cnt FROM retrieval_units GROUP BY source_kind",
    )
    .fetch_all(&pool)
    .await
    .ok()
    .map(|rows| {
        let mut parts: Vec<String> = rows
            .iter()
            .map(|row| {
                let kind: String = row.get("source_kind");
                let count: i64 = row.get("cnt");
                format!("{kind}={count}")
            })
            .collect();
        parts.sort();
        parts.join(",")
    })
    .unwrap_or_default();

    let ratio = if total_units <= 0 {
        0.0
    } else {
        let total_u32 = u32::try_from(total_units).unwrap_or(u32::MAX).max(1);
        let contradicted_u32 = u32::try_from(contradicted_units).unwrap_or(u32::MAX);
        f64::from(contradicted_u32) / f64::from(total_u32)
    };

    vec![
        format!(
            "signal stats: total_units={total_units}, raw_units={raw_units}, demoted_units={demoted_units}"
        ),
        format!(
            "signal stats: promotion_breakdown candidate={candidate_units}, promoted={promoted_units}, demoted={demoted_units}"
        ),
        format!("signal stats: ttl_expired_units={ttl_expired_units}"),
        format!("signal stats: source_kind_breakdown={source_kind_breakdown}"),
        format!(
            "signal stats: contradicted_units={contradicted_units}, contradiction_ratio={ratio:.3}"
        ),
    ]
}
