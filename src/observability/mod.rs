pub mod log;
pub mod multi;
pub mod noop;
pub mod otel;
pub mod prometheus;
pub mod traits;

pub use self::log::LogObserver;
pub use self::otel::OtelObserver;
pub use self::prometheus::PrometheusObserver;
pub use noop::NoopObserver;
pub use traits::{Observer, ObserverEvent};

use crate::config::ObservabilityConfig;

/// Factory: create the right observer from config
pub fn create_observer(config: &ObservabilityConfig) -> Box<dyn Observer> {
    match config.backend.as_str() {
        "log" => Box::new(LogObserver::new()),
        "prometheus" => Box::new(PrometheusObserver::new()),
        "otel" => Box::new(OtelObserver::new()),
        "none" | "noop" => Box::new(NoopObserver),
        _ => {
            tracing::warn!(
                "Unknown observability backend '{}', falling back to noop",
                config.backend
            );
            Box::new(NoopObserver)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::traits::ObserverMetric;
    use std::time::Duration;

    #[test]
    fn factory_none_returns_noop() {
        let cfg = ObservabilityConfig {
            backend: "none".into(),
        };
        assert_eq!(create_observer(&cfg).name(), "noop");
    }

    #[test]
    fn factory_noop_returns_noop() {
        let cfg = ObservabilityConfig {
            backend: "noop".into(),
        };
        assert_eq!(create_observer(&cfg).name(), "noop");
    }

    #[test]
    fn factory_log_returns_log() {
        let cfg = ObservabilityConfig {
            backend: "log".into(),
        };
        assert_eq!(create_observer(&cfg).name(), "log");
    }

    #[test]
    fn factory_prometheus_returns_prometheus() {
        let cfg = ObservabilityConfig {
            backend: "prometheus".into(),
        };
        assert_eq!(create_observer(&cfg).name(), "prometheus");
    }

    #[test]
    fn factory_otel_returns_otel() {
        let cfg = ObservabilityConfig {
            backend: "otel".into(),
        };
        assert_eq!(create_observer(&cfg).name(), "otel");
    }

    #[test]
    fn factory_expanded_backends_smoke_paths() {
        let prometheus = create_observer(&ObservabilityConfig {
            backend: "prometheus".into(),
        });
        prometheus.record_event(&ObserverEvent::HeartbeatTick);
        prometheus.record_metric(&ObserverMetric::QueueDepth(1));
        prometheus.flush();

        let otel = create_observer(&ObservabilityConfig {
            backend: "otel".into(),
        });
        otel.record_event(&ObserverEvent::AgentEnd {
            duration: Duration::from_secs(1),
            tokens_used: Some(123),
        });
        otel.record_metric(&ObserverMetric::TokensUsed(123));
        otel.flush();
    }

    #[test]
    fn factory_unknown_falls_back_to_noop() {
        let cfg = ObservabilityConfig {
            backend: "xyzzy_garbage_123".into(),
        };
        assert_eq!(create_observer(&cfg).name(), "noop");
    }

    #[test]
    fn factory_empty_string_falls_back_to_noop() {
        let cfg = ObservabilityConfig {
            backend: String::new(),
        };
        assert_eq!(create_observer(&cfg).name(), "noop");
    }
}
