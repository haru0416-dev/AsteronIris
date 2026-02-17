use super::traits::{AutonomyLifecycleSignal, Observer, ObserverEvent, ObserverMetric};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct PrometheusObserver {
    event_count: AtomicU64,
    metric_count: AtomicU64,
    error_count: AtomicU64,
    autonomy_lifecycle_total: AtomicU64,
    autonomy_ingested_count: AtomicU64,
    autonomy_deduplicated_count: AtomicU64,
    autonomy_promoted_count: AtomicU64,
    autonomy_contradiction_count: AtomicU64,
    autonomy_intent_created_count: AtomicU64,
    autonomy_intent_policy_allowed_count: AtomicU64,
    autonomy_intent_policy_denied_count: AtomicU64,
    autonomy_intent_dispatched_count: AtomicU64,
    autonomy_intent_execution_blocked_count: AtomicU64,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutonomyMetricCounts {
    pub total: u64,
    pub ingested: u64,
    pub deduplicated: u64,
    pub promoted: u64,
    pub contradiction_detected: u64,
    pub intent_created: u64,
    pub intent_policy_allowed: u64,
    pub intent_policy_denied: u64,
    pub intent_dispatched: u64,
    pub intent_execution_blocked: u64,
}

impl PrometheusObserver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_count: AtomicU64::new(0),
            metric_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            autonomy_lifecycle_total: AtomicU64::new(0),
            autonomy_ingested_count: AtomicU64::new(0),
            autonomy_deduplicated_count: AtomicU64::new(0),
            autonomy_promoted_count: AtomicU64::new(0),
            autonomy_contradiction_count: AtomicU64::new(0),
            autonomy_intent_created_count: AtomicU64::new(0),
            autonomy_intent_policy_allowed_count: AtomicU64::new(0),
            autonomy_intent_policy_denied_count: AtomicU64::new(0),
            autonomy_intent_dispatched_count: AtomicU64::new(0),
            autonomy_intent_execution_blocked_count: AtomicU64::new(0),
        }
    }

    fn record_autonomy_signal(&self, signal: AutonomyLifecycleSignal) {
        self.autonomy_lifecycle_total
            .fetch_add(1, Ordering::Relaxed);

        match signal {
            AutonomyLifecycleSignal::Ingested => {
                self.autonomy_ingested_count.fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::Deduplicated => {
                self.autonomy_deduplicated_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::Promoted => {
                self.autonomy_promoted_count.fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::ContradictionDetected => {
                self.autonomy_contradiction_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::IntentCreated => {
                self.autonomy_intent_created_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::IntentPolicyAllowed => {
                self.autonomy_intent_policy_allowed_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::IntentPolicyDenied => {
                self.autonomy_intent_policy_denied_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::IntentDispatched => {
                self.autonomy_intent_dispatched_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            AutonomyLifecycleSignal::IntentExecutionBlocked => {
                self.autonomy_intent_execution_blocked_count
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[cfg(test)]
    fn snapshot_counts(&self) -> (u64, u64, u64) {
        (
            self.event_count.load(Ordering::Relaxed),
            self.metric_count.load(Ordering::Relaxed),
            self.error_count.load(Ordering::Relaxed),
        )
    }

    #[cfg(test)]
    pub fn snapshot_autonomy_counts(&self) -> AutonomyMetricCounts {
        AutonomyMetricCounts {
            total: self.autonomy_lifecycle_total.load(Ordering::Relaxed),
            ingested: self.autonomy_ingested_count.load(Ordering::Relaxed),
            deduplicated: self.autonomy_deduplicated_count.load(Ordering::Relaxed),
            promoted: self.autonomy_promoted_count.load(Ordering::Relaxed),
            contradiction_detected: self.autonomy_contradiction_count.load(Ordering::Relaxed),
            intent_created: self.autonomy_intent_created_count.load(Ordering::Relaxed),
            intent_policy_allowed: self
                .autonomy_intent_policy_allowed_count
                .load(Ordering::Relaxed),
            intent_policy_denied: self
                .autonomy_intent_policy_denied_count
                .load(Ordering::Relaxed),
            intent_dispatched: self
                .autonomy_intent_dispatched_count
                .load(Ordering::Relaxed),
            intent_execution_blocked: self
                .autonomy_intent_execution_blocked_count
                .load(Ordering::Relaxed),
        }
    }
}

impl Observer for PrometheusObserver {
    fn record_event(&self, event: &ObserverEvent) {
        self.event_count.fetch_add(1, Ordering::Relaxed);
        if matches!(event, ObserverEvent::Error { .. }) {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        self.metric_count.fetch_add(1, Ordering::Relaxed);
        if let ObserverMetric::AutonomyLifecycle(signal) = metric {
            self.record_autonomy_signal(*signal);
        }
    }

    fn flush(&self) {
        tracing::debug!(
            events_total = self.event_count.load(Ordering::Relaxed),
            metrics_total = self.metric_count.load(Ordering::Relaxed),
            errors_total = self.error_count.load(Ordering::Relaxed),
            autonomy_total = self.autonomy_lifecycle_total.load(Ordering::Relaxed),
            "observer.prometheus.flush"
        );
    }

    fn name(&self) -> &str {
        "prometheus"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn prometheus_observer_name() {
        assert_eq!(PrometheusObserver::new().name(), "prometheus");
    }

    #[test]
    fn prometheus_observer_smoke_and_counts() {
        let obs = PrometheusObserver::new();

        obs.record_event(&ObserverEvent::HeartbeatTick);
        obs.record_event(&ObserverEvent::Error {
            component: "health".into(),
            message: "degraded".into(),
        });
        obs.record_metric(&ObserverMetric::AutonomyLifecycle(
            AutonomyLifecycleSignal::IntentCreated,
        ));
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::from_millis(10)));
        obs.flush();

        assert_eq!(obs.snapshot_counts(), (2, 2, 1));
        let autonomy = obs.snapshot_autonomy_counts();
        assert_eq!(autonomy.total, 1);
        assert_eq!(autonomy.intent_created, 1);
    }
}
