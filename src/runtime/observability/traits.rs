use std::time::Duration;

/// Events the observer can record
#[derive(Debug, Clone)]
pub enum ObserverEvent {
    AgentStart {
        provider: String,
        model: String,
    },
    AgentEnd {
        duration: Duration,
        tokens_used: Option<u64>,
    },
    ToolCall {
        tool: String,
        duration: Duration,
        success: bool,
    },
    ChannelMessage {
        channel: String,
        direction: String,
    },
    HeartbeatTick,
    Error {
        component: String,
        message: String,
    },
}

/// Numeric metrics
#[derive(Debug, Clone)]
pub enum ObserverMetric {
    RequestLatency(Duration),
    TokensUsed(u64),
    ActiveSessions(u64),
    QueueDepth(u64),
    SignalIngestTotal { source_kind: String },
    SignalDedupDropTotal { source_kind: String },
    BeliefPromotionTotal { count: u64 },
    ContradictionMarkTotal { count: u64 },
    StaleTrendPurgeTotal { count: u64 },
    SignalTierSnapshot { tier: String, count: u64 },
    PromotionStatusSnapshot { status: String, count: u64 },
    MemorySloViolation,
    AutonomyLifecycle(AutonomyLifecycleSignal),
    MemoryLifecycle(MemoryLifecycleSignal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyLifecycleSignal {
    Ingested,
    Deduplicated,
    Promoted,
    ContradictionDetected,
    ModeTransition,
    IntentCreated,
    IntentPolicyAllowed,
    IntentPolicyDenied,
    IntentDispatched,
    IntentExecutionBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLifecycleSignal {
    ConsolidationStarted,
    ConsolidationCompleted,
    ConflictDetected,
    ConflictResolved,
    RevocationApplied,
    GovernanceInspect,
    GovernanceExport,
    GovernanceDelete,
}

/// Core observability trait — implement for any backend
pub trait Observer: Send + Sync {
    /// Record a discrete event
    fn record_event(&self, event: &ObserverEvent);

    /// Record a numeric metric
    fn record_metric(&self, metric: &ObserverMetric);

    fn record_autonomy_lifecycle(&self, signal: AutonomyLifecycleSignal) {
        self.record_metric(&ObserverMetric::AutonomyLifecycle(signal));
    }

    fn record_memory_lifecycle(&self, signal: MemoryLifecycleSignal) {
        self.record_metric(&ObserverMetric::MemoryLifecycle(signal));
    }

    /// Flush any buffered data (no-op for most backends)
    fn flush(&self) {}

    /// Human-readable name of this observer
    fn name(&self) -> &str;
}
