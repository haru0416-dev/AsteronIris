mod command;
mod path;
mod tenant;
mod trackers;
mod types;

#[allow(unused_imports)]
pub use tenant::{
    TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR, TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR,
    TenantPolicyContext,
};
pub use trackers::{ActionTracker, CostTracker, EntityRateLimiter, RateLimitError};
pub use types::{ActionPolicyVerdict, AutonomyLevel, ExternalActionExecution};

use std::path::{Path, PathBuf};

const ACTION_LIMIT_EXCEEDED_ERROR: &str = "blocked by security policy: action limit exceeded";
const COST_LIMIT_EXCEEDED_ERROR: &str = "blocked by security policy: daily cost limit exceeded";

/// Security policy enforced on all tool executions
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    pub autonomy: AutonomyLevel,
    pub external_action_execution: ExternalActionExecution,
    pub workspace_dir: PathBuf,
    pub workspace_only: bool,
    pub allowed_commands: Vec<String>,
    pub forbidden_paths: Vec<String>,
    pub max_actions_per_hour: u32,
    pub max_cost_per_day_cents: u32,
    pub tracker: ActionTracker,
    pub cost_tracker: CostTracker,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            autonomy: AutonomyLevel::Supervised,
            external_action_execution: ExternalActionExecution::Disabled,
            workspace_dir: PathBuf::from("."),
            workspace_only: true,
            allowed_commands: crate::security::default_allowed_commands(),
            forbidden_paths: crate::security::default_forbidden_paths(),
            max_actions_per_hour: 20,
            max_cost_per_day_cents: 500,
            tracker: ActionTracker::new(),
            cost_tracker: CostTracker::new(),
        }
    }
}

impl SecurityPolicy {
    /// Check if autonomy level permits any action at all
    pub fn can_act(&self) -> bool {
        self.autonomy != AutonomyLevel::ReadOnly
    }

    /// Record an action and check if the rate limit has been exceeded.
    /// Returns `true` if the action is allowed, `false` if rate-limited.
    pub fn record_action(&self) -> bool {
        let count = self.tracker.record();
        count <= self.max_actions_per_hour as usize
    }

    /// Check if the rate limit would be exceeded without recording.
    pub fn is_rate_limited(&self) -> bool {
        self.tracker.count() >= self.max_actions_per_hour as usize
    }

    pub fn consume_action_and_cost(&self, estimated_cost_cents: u32) -> Result<(), &'static str> {
        if !self.record_action() {
            return Err(ACTION_LIMIT_EXCEEDED_ERROR);
        }

        if !self
            .cost_tracker
            .record(estimated_cost_cents, self.max_cost_per_day_cents)
        {
            return Err(COST_LIMIT_EXCEEDED_ERROR);
        }

        Ok(())
    }

    /// Build from config sections
    pub fn from_config(
        autonomy_config: &crate::config::AutonomyConfig,
        workspace_dir: &Path,
    ) -> Self {
        Self {
            autonomy: autonomy_config.effective_autonomy_level(),
            external_action_execution: autonomy_config.external_action_execution,
            workspace_dir: workspace_dir.to_path_buf(),
            workspace_only: autonomy_config.workspace_only,
            allowed_commands: autonomy_config.allowed_commands.clone(),
            forbidden_paths: autonomy_config.forbidden_paths.clone(),
            max_actions_per_hour: autonomy_config.max_actions_per_hour,
            max_cost_per_day_cents: autonomy_config.max_cost_per_day_cents,
            tracker: ActionTracker::new(),
            cost_tracker: CostTracker::new(),
        }
    }
}

#[cfg(test)]
mod tests;
