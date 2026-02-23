use serde::{Deserialize, Serialize};
use strum::Display;

#[derive(Debug, Clone, Serialize, Deserialize, Display, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AgentRole {
    Planner,
    Executor,
    Reviewer,
    Critic,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    pub role: AgentRole,
    #[serde(default)]
    pub system_prompt_override: Option<String>,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub temperature_override: Option<f64>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    pub run_id: String,
    pub role: AgentRole,
    pub config: RoleConfig,
    pub assigned_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_role_serializes_snake_case() {
        let role = AgentRole::Planner;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"planner\"");
        let role2 = AgentRole::Reviewer;
        let json2 = serde_json::to_string(&role2).unwrap();
        assert_eq!(json2, "\"reviewer\"");
    }
}
