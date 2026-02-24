use crate::config::Config;
use anyhow::Result;
use chrono::Utc;
use std::fs;

#[derive(Debug, Clone, serde::Serialize)]
pub struct EvolutionRecommendation {
    pub id: String,
    pub reason: String,
    pub risk: String,
    pub proposed: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EvolutionReport {
    pub generated_at: String,
    pub apply_mode: bool,
    pub observations: Vec<String>,
    pub recommendations: Vec<EvolutionRecommendation>,
    pub applied_changes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Proposal {
    SchedulerPollSecs(u64),
    ProviderBackoffMs(u64),
    ProviderRetries(u32),
    #[allow(dead_code)]
    EnsureFallback(String),
}

pub fn run_cycle(config: &Config, apply: bool) -> Result<()> {
    let daemon_snapshot = load_daemon_snapshot(config);

    let mut observations = collect_observations(config, daemon_snapshot.as_ref());
    let proposals = build_proposals(config, daemon_snapshot.as_ref(), &mut observations);
    let recommendations = proposals
        .iter()
        .map(proposal_to_recommendation)
        .collect::<Vec<_>>();

    let mut updated = config.clone();
    let applied_changes = if apply {
        let changes = apply_proposals(&mut updated, &proposals);
        if !changes.is_empty() {
            updated.validate_autonomy_controls()?;
            updated.save()?;
        }
        changes
    } else {
        Vec::new()
    };

    let report = EvolutionReport {
        generated_at: Utc::now().to_rfc3339(),
        apply_mode: apply,
        observations,
        recommendations,
        applied_changes,
    };

    let report_path = write_report(config, &report)?;
    println!("◆ Self-evolution cycle complete");
    println!("Mode: {}", if apply { "apply" } else { "dry-run" });
    println!("Recommendations: {}", report.recommendations.len());
    println!("Applied changes: {}", report.applied_changes.len());
    println!("Report: {}", report_path.display());
    Ok(())
}

fn load_daemon_snapshot(config: &Config) -> Option<serde_json::Value> {
    let path = config
        .config_path
        .parent()
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
        .join("daemon_state.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn collect_observations(
    config: &Config,
    daemon_snapshot: Option<&serde_json::Value>,
) -> Vec<String> {
    let mut observations = Vec::new();
    observations.push(format!(
        "provider={} model={} retries={} backoff_ms={}",
        config.default_provider.as_deref().unwrap_or("unset"),
        config.default_model.as_deref().unwrap_or("unset"),
        config.reliability.provider_retries,
        config.reliability.provider_backoff_ms
    ));

    // TODO: Port security::auth::AuthProfileStore to v2 for profile error tracking.
    // For now, skip auth profile analysis.
    observations.push("auth_profiles: not yet ported to v2".to_string());

    if let Some(snapshot) = daemon_snapshot
        && let Some(components) = snapshot
            .get("components")
            .and_then(serde_json::Value::as_object)
        && let Some(scheduler) = components.get("scheduler")
    {
        let status = scheduler
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        observations.push(format!("daemon.scheduler.status={status}"));
    } else {
        observations.push("daemon snapshot unavailable".to_string());
    }

    observations
}

fn build_proposals(
    config: &Config,
    daemon_snapshot: Option<&serde_json::Value>,
    observations: &mut Vec<String>,
) -> Vec<Proposal> {
    let mut proposals = Vec::new();

    if config.reliability.scheduler_poll_secs < 5 || config.reliability.scheduler_poll_secs > 120 {
        proposals.push(Proposal::SchedulerPollSecs(15));
    }

    if config.reliability.provider_backoff_ms < 200
        || config.reliability.provider_backoff_ms > 10_000
    {
        proposals.push(Proposal::ProviderBackoffMs(500));
    }

    if config.reliability.provider_retries > 4 {
        proposals.push(Proposal::ProviderRetries(2));
    }

    let provider = config.default_provider.as_deref().unwrap_or_default();
    let fallback_exists = !config.reliability.fallback_providers.is_empty();
    // TODO: Once AuthProfileStore is ported, check total_errors >= 3 before recommending fallback
    if !fallback_exists && matches!(provider, "openai" | "openai-codex" | "anthropic") {
        observations
            .push("no fallback provider configured; diversification recommended".to_string());
    }

    if let Some(snapshot) = daemon_snapshot
        && let Some(components) = snapshot
            .get("components")
            .and_then(serde_json::Value::as_object)
        && let Some(scheduler) = components.get("scheduler")
    {
        let stale_scheduler = scheduler
            .get("status")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| status == "error");
        if stale_scheduler && config.reliability.scheduler_poll_secs > 30 {
            proposals.push(Proposal::SchedulerPollSecs(15));
        }
    }

    proposals
}

fn proposal_to_recommendation(proposal: &Proposal) -> EvolutionRecommendation {
    match proposal {
        Proposal::SchedulerPollSecs(value) => EvolutionRecommendation {
            id: "scheduler_poll_normalize".to_string(),
            reason: "scheduler polling interval is outside safe operating band".to_string(),
            risk: "low".to_string(),
            proposed: format!("set reliability.scheduler_poll_secs = {value}"),
        },
        Proposal::ProviderBackoffMs(value) => EvolutionRecommendation {
            id: "provider_backoff_normalize".to_string(),
            reason: "provider retry backoff is outside bounded range".to_string(),
            risk: "low".to_string(),
            proposed: format!("set reliability.provider_backoff_ms = {value}"),
        },
        Proposal::ProviderRetries(value) => EvolutionRecommendation {
            id: "provider_retries_normalize".to_string(),
            reason: "provider retries exceed safe budget and can increase cost/latency".to_string(),
            risk: "low".to_string(),
            proposed: format!("set reliability.provider_retries = {value}"),
        },
        Proposal::EnsureFallback(provider) => EvolutionRecommendation {
            id: "fallback_provider_add".to_string(),
            reason: "repeated auth/provider failures detected without fallback provider"
                .to_string(),
            risk: "low".to_string(),
            proposed: format!("append reliability.fallback_providers += \"{provider}\""),
        },
    }
}

fn apply_proposals(config: &mut Config, proposals: &[Proposal]) -> Vec<String> {
    let mut changes = Vec::new();
    for proposal in proposals {
        match proposal {
            Proposal::SchedulerPollSecs(value) => {
                if config.reliability.scheduler_poll_secs != *value {
                    config.reliability.scheduler_poll_secs = *value;
                    changes.push(format!("reliability.scheduler_poll_secs -> {value}"));
                }
            }
            Proposal::ProviderBackoffMs(value) => {
                if config.reliability.provider_backoff_ms != *value {
                    config.reliability.provider_backoff_ms = *value;
                    changes.push(format!("reliability.provider_backoff_ms -> {value}"));
                }
            }
            Proposal::ProviderRetries(value) => {
                if config.reliability.provider_retries != *value {
                    config.reliability.provider_retries = *value;
                    changes.push(format!("reliability.provider_retries -> {value}"));
                }
            }
            Proposal::EnsureFallback(provider) => {
                if !config
                    .reliability
                    .fallback_providers
                    .iter()
                    .any(|current| current == provider)
                {
                    config.reliability.fallback_providers.push(provider.clone());
                    changes.push(format!("reliability.fallback_providers += {provider}"));
                }
            }
        }
    }
    changes
}

fn write_report(config: &Config, report: &EvolutionReport) -> Result<std::path::PathBuf> {
    let reports_dir = config.workspace_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;
    let path = reports_dir.join("self_evolution_latest.json");
    let json = serde_json::to_string_pretty(report)?;
    fs::write(&path, json)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Proposal, apply_proposals, build_proposals};
    use crate::config::Config;

    #[test]
    fn build_proposals_normalizes_out_of_bounds_reliability() {
        let mut config = Config::default();
        config.reliability.scheduler_poll_secs = 1;
        config.reliability.provider_backoff_ms = 15;
        config.reliability.provider_retries = 8;

        let mut observations = Vec::new();
        let proposals = build_proposals(&config, None, &mut observations);

        assert!(proposals.contains(&Proposal::SchedulerPollSecs(15)));
        assert!(proposals.contains(&Proposal::ProviderBackoffMs(500)));
        assert!(proposals.contains(&Proposal::ProviderRetries(2)));
    }

    #[test]
    fn apply_proposals_updates_config_fields() {
        let mut config = Config::default();
        config.reliability.scheduler_poll_secs = 30;
        config.reliability.provider_backoff_ms = 900;
        config.reliability.provider_retries = 4;
        config.reliability.fallback_providers.clear();
        let proposals = vec![
            Proposal::SchedulerPollSecs(15),
            Proposal::ProviderBackoffMs(500),
            Proposal::ProviderRetries(2),
            Proposal::EnsureFallback("openrouter".to_string()),
        ];

        let changes = apply_proposals(&mut config, &proposals);
        assert_eq!(changes.len(), 4);
        assert_eq!(config.reliability.scheduler_poll_secs, 15);
        assert_eq!(config.reliability.provider_backoff_ms, 500);
        assert_eq!(config.reliability.provider_retries, 2);
        assert!(
            config
                .reliability
                .fallback_providers
                .iter()
                .any(|provider| provider == "openrouter")
        );
    }
}
