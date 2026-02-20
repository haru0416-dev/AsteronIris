use crate::config::Config;

#[allow(clippy::too_many_lines)]
pub fn render_status(config: &Config) -> String {
    let mut lines = vec![
        format!("◆ {}", t!("status.title")),
        String::new(),
        format!("{}     {}", t!("status.version"), env!("CARGO_PKG_VERSION")),
        format!(
            "{}   {}",
            t!("status.workspace"),
            config.workspace_dir.display()
        ),
        format!(
            "{}      {}",
            t!("status.config"),
            config.config_path.display()
        ),
        String::new(),
        format!(
            "  {}      {}",
            t!("status.provider"),
            config.default_provider.as_deref().unwrap_or("openrouter")
        ),
        format!(
            "   {}         {}",
            t!("status.model"),
            config.default_model.as_deref().unwrap_or("(default)")
        ),
        format!(
            "  {}  {}",
            t!("status.observability"),
            config.observability.backend
        ),
        format!(
            "  {}      {:?}",
            t!("status.autonomy"),
            config.autonomy.level
        ),
        format!(
            "   {} {}",
            t!("status.external_actions"),
            match config.autonomy.external_action_execution {
                crate::security::ExternalActionExecution::Disabled => "disabled",
                crate::security::ExternalActionExecution::Enabled => "enabled",
            }
        ),
    ];

    let temperature_band = config.autonomy.selected_temperature_band();
    lines.push(format!(
        "   {} [{:.2}, {:.2}]",
        t!("status.temperature_band"),
        temperature_band.min,
        temperature_band.max
    ));
    lines.push(format!(
        "   {} {}",
        t!("status.rollout_stage"),
        match config.autonomy.rollout.stage {
            Some(crate::config::schema::AutonomyRolloutStage::ReadOnly) => "read-only",
            Some(crate::config::schema::AutonomyRolloutStage::Supervised) => "supervised",
            Some(crate::config::schema::AutonomyRolloutStage::Full) => "full",
            None => "off",
        }
    ));
    lines.push(format!(
        "   {} enabled={}, read_only_days={:?}, supervised_days={:?}",
        t!("status.rollout_policy"),
        if config.autonomy.rollout.enabled {
            "on"
        } else {
            "off"
        },
        config.autonomy.rollout.read_only_days,
        config.autonomy.rollout.supervised_days
    ));
    lines.push(format!(
        "   {} max_attempts={}, max_repair_depth={}",
        t!("status.verify_repair"),
        config.autonomy.verify_repair_max_attempts,
        config.autonomy.verify_repair_max_repair_depth
    ));
    lines.push(format!(
        "   {} {}",
        t!("status.autonomy_metrics"),
        if observability_backend_supports_autonomy_metrics(&config.observability.backend) {
            "enabled"
        } else {
            "disabled"
        }
    ));
    lines.push(format!(
        "  {}       {}",
        t!("status.runtime"),
        config.runtime.kind
    ));

    lines.push(format!(
        "  {}      {}",
        t!("status.heartbeat"),
        if config.heartbeat.enabled {
            format!("every {}min", config.heartbeat.interval_minutes)
        } else {
            "disabled".into()
        }
    ));
    lines.push(format!(
        "  {}         {} (auto-save: {})",
        t!("status.memory"),
        config.memory.backend,
        if config.memory.auto_save { "on" } else { "off" }
    ));

    let (consolidation, conflict, revocation, governance) = memory_rollout_status(config);
    lines.push(format!(
        "   {} consolidation={consolidation}, conflict={conflict}, revocation={revocation}, governance={governance}",
        t!("status.memory_rollout"),
    ));
    lines.push(format!(
        "   {} {}",
        t!("status.memory_metrics"),
        if observability_backend_supports_memory_metrics(&config.observability.backend) {
            "enabled"
        } else {
            "disabled"
        }
    ));

    lines.extend([
        String::new(),
        format!("{}", t!("status.security")),
        format!(
            "  {}    {}",
            t!("status.workspace_only"),
            config.autonomy.workspace_only
        ),
        format!(
            "  {}  {}",
            t!("status.allowed_commands"),
            config.autonomy.allowed_commands.join(", ")
        ),
        format!(
            "  {}  {}",
            t!("status.max_actions"),
            config.autonomy.max_actions_per_hour
        ),
        format!(
            "  {}      ${:.2}",
            t!("status.max_cost"),
            f64::from(config.autonomy.max_cost_per_day_cents) / 100.0
        ),
        String::new(),
        format!("{}", t!("status.channels")),
        format!("  {}", t!("status.cli_always")),
    ]);

    for (name, configured) in [
        ("Telegram", config.channels_config.telegram.is_some()),
        ("Discord", config.channels_config.discord.is_some()),
        ("Slack", config.channels_config.slack.is_some()),
        ("Webhook", config.channels_config.webhook.is_some()),
    ] {
        lines.push(format!(
            "  {name:9} {}",
            if configured {
                format!("✓ {}", t!("common.confirmed"))
            } else {
                format!("✗ {}", t!("common.not_configured"))
            }
        ));
    }

    lines.join("\n")
}

fn observability_backend_supports_autonomy_metrics(backend: &str) -> bool {
    matches!(backend, "log" | "prometheus" | "otel")
}

fn observability_backend_supports_memory_metrics(backend: &str) -> bool {
    matches!(backend, "log" | "prometheus" | "otel")
}

fn memory_rollout_status(
    config: &Config,
) -> (&'static str, &'static str, &'static str, &'static str) {
    let backend = config.memory.backend.as_str();
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

    let capability = crate::memory::capability_matrix_for_backend(backend);
    let revocation = capability.map_or("unknown", |matrix| {
        capability_support_label(matrix.forget_tombstone)
    });
    let governance = capability.map_or("unknown", |matrix| {
        capability_support_label(matrix.forget_hard)
    });

    (consolidation, conflict, revocation, governance)
}

fn capability_support_label(support: crate::memory::CapabilitySupport) -> &'static str {
    match support {
        crate::memory::CapabilitySupport::Supported => "supported",
        crate::memory::CapabilitySupport::Degraded => "degraded",
        crate::memory::CapabilitySupport::Unsupported => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::memory_rollout_status;
    use crate::config::Config;

    #[test]
    fn status_reports_memory_rollout_support() {
        let mut config = Config::default();
        config.memory.backend = "lancedb".into();
        config.memory.auto_save = true;
        config.autonomy.rollout.enabled = true;

        let rollout = memory_rollout_status(&config);
        assert_eq!(rollout.0, "on");
        assert_eq!(rollout.1, "on");
        assert_eq!(rollout.2, "degraded");
        assert_eq!(rollout.3, "supported");
    }
}
