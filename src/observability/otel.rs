use super::traits::{AutonomyLifecycleSignal, Observer, ObserverEvent, ObserverMetric};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct OtelObserver {
    event_count: AtomicU64,
    metric_count: AtomicU64,
}

impl OtelObserver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_count: AtomicU64::new(0),
            metric_count: AtomicU64::new(0),
        }
    }

    fn event_kind(event: &ObserverEvent) -> &'static str {
        match event {
            ObserverEvent::AgentStart { .. } => "agent_start",
            ObserverEvent::AgentEnd { .. } => "agent_end",
            ObserverEvent::ToolCall { .. } => "tool_call",
            ObserverEvent::ChannelMessage { .. } => "channel_message",
            ObserverEvent::HeartbeatTick => "heartbeat_tick",
            ObserverEvent::Error { .. } => "error",
        }
    }

    fn metric_kind(metric: &ObserverMetric) -> &'static str {
        match metric {
            ObserverMetric::RequestLatency(_) => "request_latency",
            ObserverMetric::TokensUsed(_) => "tokens_used",
            ObserverMetric::ActiveSessions(_) => "active_sessions",
            ObserverMetric::QueueDepth(_) => "queue_depth",
            ObserverMetric::AutonomyLifecycle(signal) => autonomy_signal_name(*signal),
        }
    }

    #[cfg(test)]
    fn snapshot_counts(&self) -> (u64, u64) {
        (
            self.event_count.load(Ordering::Relaxed),
            self.metric_count.load(Ordering::Relaxed),
        )
    }
}

fn autonomy_signal_name(signal: AutonomyLifecycleSignal) -> &'static str {
    match signal {
        AutonomyLifecycleSignal::Ingested => "autonomy_ingested",
        AutonomyLifecycleSignal::Deduplicated => "autonomy_deduplicated",
        AutonomyLifecycleSignal::Promoted => "autonomy_promoted",
        AutonomyLifecycleSignal::ContradictionDetected => "autonomy_contradiction_detected",
        AutonomyLifecycleSignal::IntentCreated => "autonomy_intent_created",
        AutonomyLifecycleSignal::IntentPolicyAllowed => "autonomy_intent_policy_allowed",
        AutonomyLifecycleSignal::IntentPolicyDenied => "autonomy_intent_policy_denied",
        AutonomyLifecycleSignal::IntentDispatched => "autonomy_intent_dispatched",
        AutonomyLifecycleSignal::IntentExecutionBlocked => "autonomy_intent_execution_blocked",
    }
}

impl Observer for OtelObserver {
    fn record_event(&self, event: &ObserverEvent) {
        self.event_count.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(event = Self::event_kind(event), "observer.otel.event");
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        self.metric_count.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(metric = Self::metric_kind(metric), "observer.otel.metric");
    }

    fn flush(&self) {
        tracing::debug!(
            events_total = self.event_count.load(Ordering::Relaxed),
            metrics_total = self.metric_count.load(Ordering::Relaxed),
            "observer.otel.flush"
        );
    }

    fn name(&self) -> &str {
        "otel"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn otel_observer_name() {
        assert_eq!(OtelObserver::new().name(), "otel");
    }

    #[test]
    fn otel_observer_smoke_and_counts() {
        let obs = OtelObserver::new();

        obs.record_event(&ObserverEvent::AgentStart {
            provider: "openrouter".into(),
            model: "gpt-5".into(),
        });
        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::from_millis(5)));
        obs.record_metric(&ObserverMetric::TokensUsed(10));
        obs.record_metric(&ObserverMetric::AutonomyLifecycle(
            AutonomyLifecycleSignal::IntentPolicyDenied,
        ));
        obs.flush();

        assert_eq!(obs.snapshot_counts(), (2, 3));
    }
}
