use crate::security::AutonomyLevel;
use crate::transport::channels::traits::Channel;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ChannelPolicy {
    pub autonomy_level: Option<AutonomyLevel>,
    pub tool_allowlist: Option<HashSet<String>>,
}

#[derive(Clone)]
pub struct ChannelEntry {
    pub name: &'static str,
    pub channel: Arc<dyn Channel>,
    pub policy: ChannelPolicy,
}

/// Effective autonomy = min(global, channel). Channel cannot escalate beyond global.
#[must_use]
pub fn min_autonomy(global: AutonomyLevel, channel: AutonomyLevel) -> AutonomyLevel {
    match (global, channel) {
        (AutonomyLevel::ReadOnly, _) | (_, AutonomyLevel::ReadOnly) => AutonomyLevel::ReadOnly,
        (AutonomyLevel::Supervised, _) | (_, AutonomyLevel::Supervised) => {
            AutonomyLevel::Supervised
        }
        (AutonomyLevel::Full, AutonomyLevel::Full) => AutonomyLevel::Full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_autonomy_read_only_wins_when_global_read_only() {
        assert_eq!(
            min_autonomy(AutonomyLevel::ReadOnly, AutonomyLevel::Full),
            AutonomyLevel::ReadOnly
        );
    }

    #[test]
    fn min_autonomy_supervised_wins_over_full() {
        assert_eq!(
            min_autonomy(AutonomyLevel::Supervised, AutonomyLevel::Full),
            AutonomyLevel::Supervised
        );
    }

    #[test]
    fn min_autonomy_full_only_when_both_full() {
        assert_eq!(
            min_autonomy(AutonomyLevel::Full, AutonomyLevel::Full),
            AutonomyLevel::Full
        );
    }

    #[test]
    fn min_autonomy_read_only_wins_when_channel_read_only() {
        assert_eq!(
            min_autonomy(AutonomyLevel::Full, AutonomyLevel::ReadOnly),
            AutonomyLevel::ReadOnly
        );
    }
}
