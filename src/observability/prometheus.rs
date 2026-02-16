use super::traits::{Observer, ObserverEvent, ObserverMetric};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct PrometheusObserver {
    event_count: AtomicU64,
    metric_count: AtomicU64,
    error_count: AtomicU64,
}

impl PrometheusObserver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            event_count: AtomicU64::new(0),
            metric_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
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
}

impl Observer for PrometheusObserver {
    fn record_event(&self, event: &ObserverEvent) {
        self.event_count.fetch_add(1, Ordering::Relaxed);
        if matches!(event, ObserverEvent::Error { .. }) {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_metric(&self, _metric: &ObserverMetric) {
        self.metric_count.fetch_add(1, Ordering::Relaxed);
    }

    fn flush(&self) {
        tracing::debug!(
            events_total = self.event_count.load(Ordering::Relaxed),
            metrics_total = self.metric_count.load(Ordering::Relaxed),
            errors_total = self.error_count.load(Ordering::Relaxed),
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
        obs.record_metric(&ObserverMetric::RequestLatency(Duration::from_millis(10)));
        obs.flush();

        assert_eq!(obs.snapshot_counts(), (2, 1, 1));
    }
}
