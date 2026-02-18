use crate::config::Config;

pub fn render_status(config: &Config) -> String {
    let mut lines = vec![
        "🦀 AsteronIris Status".to_string(),
        String::new(),
        format!("Version:     {}", env!("CARGO_PKG_VERSION")),
        format!("Workspace:   {}", config.workspace_dir.display()),
        format!("Config:      {}", config.config_path.display()),
        String::new(),
        format!(
            "🤖 Provider:      {}",
            config.default_provider.as_deref().unwrap_or("openrouter")
        ),
        format!(
            "   Model:         {}",
            config.default_model.as_deref().unwrap_or("(default)")
        ),
        format!("📊 Observability:  {}", config.observability.backend),
        format!("🛡️  Autonomy:      {:?}", config.autonomy.level),
        format!(
            "   External actions: {}",
            match config.autonomy.external_action_execution {
                crate::security::ExternalActionExecution::Disabled => "disabled",
                crate::security::ExternalActionExecution::Enabled => "enabled",
            }
        ),
    ];

    let temperature_band = config.autonomy.selected_temperature_band();
    lines.push(format!(
        "   Temperature band: [{:.2}, {:.2}]",
        temperature_band.min, temperature_band.max
    ));
    lines.push(format!(
        "   Rollout stage: {}",
        match config.autonomy.rollout.stage {
            crate::config::schema::AutonomyRolloutStage::Off => "off",
            crate::config::schema::AutonomyRolloutStage::AuditOnly => "audit-only",
            crate::config::schema::AutonomyRolloutStage::Sanitize => "sanitize",
        }
    ));
    lines.push(format!(
        "   Rollout gates: verify_repair={}, contradiction_weighting={}, intent_audit_anomaly_detection={}",
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
    lines.push(format!(
        "   Autonomy lifecycle metrics: {}",
        if observability_backend_supports_autonomy_metrics(&config.observability.backend) {
            "enabled"
        } else {
            "disabled"
        }
    ));
    lines.push(format!("⚙️  Runtime:       {}", config.runtime.kind));

    if let Some(contract_note) =
        crate::runtime::runtime_kind_contract_note(config.runtime.kind.as_str())
    {
        lines.push(format!("   Runtime contract: {contract_note}"));
    }

    lines.push(format!(
        "💓 Heartbeat:      {}",
        if config.heartbeat.enabled {
            format!("every {}min", config.heartbeat.interval_minutes)
        } else {
            "disabled".into()
        }
    ));
    lines.push(format!(
        "🧠 Memory:         {} (auto-save: {})",
        config.memory.backend,
        if config.memory.auto_save { "on" } else { "off" }
    ));

    let (consolidation, conflict, revocation, governance) = memory_rollout_status(config);
    lines.push(format!(
        "   Memory rollout: consolidation={consolidation}, conflict={conflict}, revocation={revocation}, governance={governance}"
    ));
    lines.push(format!(
        "   Memory lifecycle metrics: {}",
        if observability_backend_supports_memory_metrics(&config.observability.backend) {
            "enabled"
        } else {
            "disabled"
        }
    ));

    lines.extend([
        String::new(),
        "Security:".to_string(),
        format!("  Workspace only:    {}", config.autonomy.workspace_only),
        format!(
            "  Allowed commands:  {}",
            config.autonomy.allowed_commands.join(", ")
        ),
        format!(
            "  Max actions/hour:  {}",
            config.autonomy.max_actions_per_hour
        ),
        format!(
            "  Max cost/day:      ${:.2}",
            f64::from(config.autonomy.max_cost_per_day_cents) / 100.0
        ),
        String::new(),
        "Channels:".to_string(),
        "  CLI:      ✅ always".to_string(),
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
                "✅ configured"
            } else {
                "❌ not configured"
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
    let conflict = if backend != "none" && config.autonomy.rollout.contradiction_weighting_enabled {
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
        config.autonomy.rollout.contradiction_weighting_enabled = true;

        let rollout = memory_rollout_status(&config);
        assert_eq!(rollout.0, "on");
        assert_eq!(rollout.1, "on");
        assert_eq!(rollout.2, "degraded");
        assert_eq!(rollout.3, "supported");
    }
}
