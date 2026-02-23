use super::SubagentRunStatus;
use super::roles::{AgentRole, RoleAssignment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedContext {
    pub messages: Vec<ContextMessage>,
    pub artifacts: HashMap<String, serde_json::Value>,
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl Default for SharedContext {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            artifacts: HashMap::new(),
            metadata: serde_json::Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: AgentRole,
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationSession {
    pub session_id: String,
    pub roles: Vec<RoleAssignment>,
    pub shared_context: SharedContext,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchResult {
    pub run_id: String,
    pub role: AgentRole,
    pub status: SubagentRunStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedResult {
    pub session_id: String,
    pub results: Vec<DispatchResult>,
    pub total_elapsed_ms: u64,
    pub all_succeeded: bool,
}
