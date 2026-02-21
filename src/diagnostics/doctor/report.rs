use crate::config::Config;
use crate::intelligence::memory::CapabilitySupport;
use crate::security::ExternalActionExecution;
use chrono::{DateTime, Utc};

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
    let mut lines = Vec::with_capacity(5);
    let backend = config.memory.backend.as_str();
    let capability = crate::intelligence::memory::capability_matrix_for_backend(backend);

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

pub(crate) fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}
