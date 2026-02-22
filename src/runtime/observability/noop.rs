use super::traits::{Observer, ObserverEvent, ObserverMetric};

/// Zero-overhead observer — all methods compile to nothing
pub struct NoopObserver;

impl Observer for NoopObserver {
    #[inline(always)]
    fn record_event(&self, _event: &ObserverEvent) {}

    #[inline(always)]
    fn record_metric(&self, _metric: &ObserverMetric) {}

    fn name(&self) -> &str {
        "noop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::observability::traits::{AutonomyLifecycleSignal, MemoryLifecycleSignal};
    use std::time::Duration;

    #[test]
    fn noop_name() {
        assert_eq!(NoopObserver.name(), "noop");
    }

    #[test]
    fn noop_record_event_does_not_panic() {
        let obs = NoopObserver;
        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.record_event(&ObserverEvent::AgentStart {
            provider: "test".into(),
            model: "test".into(),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            duration: Duration::from_millis(100),
            tokens_used: Some(42),
        });
        obs.record_event(&ObserverEvent::AgentEnd {
            duration: Duration::ZERO,
            tokens_used: None,
        });
        obs.record_event(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            duration: Duration::from_secs(1),
            success: true,
        });
        obs.record_event(&ObserverEvent::ChannelMessage {
            channel: "cli".into(),
            direction: "inbound".into(),
        });
        obs.record_event(&ObserverEvent::Error {
            component: "test".into(),
            message: "boom".into(),
        });
    }

    #[test]
    fn noop_record_metric_does_not_panic() {
        let obs = NoopObserver;
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::from_millis(50)));
        obs.record_metric(&ObserverMetric::TokensUsed(1000));
        obs.record_metric(&ObserverMetric::ActiveSessions(5));
        obs.record_metric(&ObserverMetric::QueueDepth(0));
        obs.record_metric(&ObserverMetric::AutonomyLifecycle(
            AutonomyLifecycleSignal::IntentExecutionBlocked,
        ));
        obs.record_metric(&ObserverMetric::MemoryLifecycle(
            MemoryLifecycleSignal::RevocationApplied,
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

    #[test]
    fn noop_flush_does_not_panic() {
        NoopObserver.flush();
    }
}
