use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// "none" | "log" | "prometheus" | "otel"
    pub backend: String,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            backend: "none".into(),
        }
    }
}
