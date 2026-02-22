use super::traits::{
    AutonomyLifecycleSignal, MemoryLifecycleSignal, Observer, ObserverEvent, ObserverMetric,
};
use tracing::info;

/// Log-based observer — uses tracing, zero external deps
pub struct LogObserver;

impl LogObserver {
    pub fn new() -> Self {
        Self
    }
}

impl Observer for LogObserver {
    fn record_event(&self, event: &ObserverEvent) {
        match event {
            ObserverEvent::AgentStart { provider, model } => {
                info!(provider = %provider, model = %model, "agent.start");
            }
            ObserverEvent::AgentEnd {
                duration,
                tokens_used,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                info!(duration_ms = ms, tokens = ?tokens_used, "agent.end");
            }
            ObserverEvent::ToolCall {
                tool,
                duration,
                success,
            } => {
                let ms = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
                info!(tool = %tool, duration_ms = ms, success = success, "tool.call");
            }
            ObserverEvent::ChannelMessage { channel, direction } => {
                info!(channel = %channel, direction = %direction, "channel.message");
            }
            ObserverEvent::HeartbeatTick => {
                info!("heartbeat.tick");
            }
            ObserverEvent::Error { component, message } => {
                info!(component = %component, error = %message, "error");
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        match metric {
            ObserverMetric::RequestLatency(d) => {
                let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
                info!(latency_ms = ms, "metric.request_latency");
            }
            ObserverMetric::TokensUsed(t) => {
                info!(tokens = t, "metric.tokens_used");
            }
            ObserverMetric::ActiveSessions(s) => {
                info!(sessions = s, "metric.active_sessions");
            }
            ObserverMetric::QueueDepth(d) => {
                info!(depth = d, "metric.queue_depth");
            }
            ObserverMetric::SignalIngestTotal { source_kind } => {
                info!(source_kind = %source_kind, "metric.signal_ingest_total");
            }
            ObserverMetric::SignalDedupDropTotal { source_kind } => {
                info!(source_kind = %source_kind, "metric.signal_dedup_drop_total");
            }
            ObserverMetric::BeliefPromotionTotal { count } => {
                info!(count = count, "metric.belief_promotion_total");
            }
            ObserverMetric::ContradictionMarkTotal { count } => {
                info!(count = count, "metric.contradiction_mark_total");
            }
            ObserverMetric::StaleTrendPurgeTotal { count } => {
                info!(count = count, "metric.stale_trend_purge_total");
            }
            ObserverMetric::SignalTierSnapshot { tier, count } => {
                info!(tier = %tier, count = count, "metric.signal_tier_snapshot");
            }
            ObserverMetric::PromotionStatusSnapshot { status, count } => {
                info!(status = %status, count = count, "metric.promotion_status_snapshot");
            }
            ObserverMetric::MemorySloViolation => {
                info!("metric.memory_slo_violation");
            }
            ObserverMetric::AutonomyLifecycle(signal) => {
                info!(signal = %autonomy_signal_name(*signal), "metric.autonomy_lifecycle");
            }
            ObserverMetric::MemoryLifecycle(signal) => {
                info!(signal = %memory_signal_name(*signal), "metric.memory_lifecycle");
            }
        }
    }

    fn name(&self) -> &str {
        "log"
    }
}

fn autonomy_signal_name(signal: AutonomyLifecycleSignal) -> &'static str {
    match signal {
        AutonomyLifecycleSignal::Ingested => "ingested",
        AutonomyLifecycleSignal::Deduplicated => "deduplicated",
        AutonomyLifecycleSignal::Promoted => "promoted",
        AutonomyLifecycleSignal::ContradictionDetected => "contradiction_detected",
        AutonomyLifecycleSignal::ModeTransition => "mode_transition",
        AutonomyLifecycleSignal::IntentCreated => "intent_created",
        AutonomyLifecycleSignal::IntentPolicyAllowed => "intent_policy_allowed",
        AutonomyLifecycleSignal::IntentPolicyDenied => "intent_policy_denied",
        AutonomyLifecycleSignal::IntentDispatched => "intent_dispatched",
        AutonomyLifecycleSignal::IntentExecutionBlocked => "intent_execution_blocked",
    }
}

fn memory_signal_name(signal: MemoryLifecycleSignal) -> &'static str {
    match signal {
        MemoryLifecycleSignal::ConsolidationStarted => "consolidation_started",
        MemoryLifecycleSignal::ConsolidationCompleted => "consolidation_completed",
        MemoryLifecycleSignal::ConflictDetected => "conflict_detected",
        MemoryLifecycleSignal::ConflictResolved => "conflict_resolved",
        MemoryLifecycleSignal::RevocationApplied => "revocation_applied",
        MemoryLifecycleSignal::GovernanceInspect => "governance_inspect",
        MemoryLifecycleSignal::GovernanceExport => "governance_export",
        MemoryLifecycleSignal::GovernanceDelete => "governance_delete",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn log_observer_name() {
        assert_eq!(LogObserver::new().name(), "log");
    }

    #[test]
    fn log_observer_all_events_no_panic() {
        let obs = LogObserver::new();
        obs.record_event(&ObserverEvent::AgentStart {
            provider: "openrouter".into(),
            model: "claude-sonnet".into(),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            duration: Duration::from_millis(500),
            tokens_used: Some(100),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            duration: Duration::ZERO,
            tokens_used: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            duration: Duration::from_millis(10),
            success: false,
        });
        obs.record_event(&ObserverEvent::ChannelMessage {
            channel: "telegram".into(),
            direction: "outbound".into(),
        });
        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.record_event(&ObserverEvent::Error {
            component: "provider".into(),
            message: "timeout".into(),
        });
    }

    #[test]
    fn log_observer_all_metrics_no_panic() {
        let obs = LogObserver::new();
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::from_secs(2)));
        obs.record_metric(&ObserverMetric::TokensUsed(0));
        obs.record_metric(&ObserverMetric::TokensUsed(u64::MAX));
        obs.record_metric(&ObserverMetric::ActiveSessions(1));
        obs.record_metric(&ObserverMetric::QueueDepth(999));
        obs.record_metric(&ObserverMetric::AutonomyLifecycle(
            AutonomyLifecycleSignal::IntentCreated,
        ));
        obs.record_metric(&ObserverMetric::MemoryLifecycle(
            MemoryLifecycleSignal::ConsolidationCompleted,
        ));
        obs.record_metric(&ObserverMetric::SignalTierSnapshot {
            tier: "raw".to_string(),
            count: 3,
        });
        obs.record_metric(&ObserverMetric::PromotionStatusSnapshot {
            status: "promoted".to_string(),
            count: 2,
        });
        obs.record_metric(&ObserverMetric::BeliefPromotionTotal { count: 2 });
        obs.record_metric(&ObserverMetric::ContradictionMarkTotal { count: 1 });
        obs.record_metric(&ObserverMetric::StaleTrendPurgeTotal { count: 4 });
        obs.record_metric(&ObserverMetric::MemorySloViolation);
    }
}
