use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Planner,
    Executor,
    Reviewer,
    Critic,
    Custom(String),
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Planner => write!(f, "planner"),
            Self::Executor => write!(f, "executor"),
            Self::Reviewer => write!(f, "reviewer"),
            Self::Critic => write!(f, "critic"),
            Self::Custom(name) => write!(f, "{name}"),
        }
    }
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
    fn agent_role_serializes_to_snake_case() {
        let planner = serde_json::to_string(&AgentRole::Planner).unwrap();
        assert_eq!(planner, "\"planner\"");

        let executor = serde_json::to_string(&AgentRole::Executor).unwrap();
        assert_eq!(executor, "\"executor\"");

        let reviewer = serde_json::to_string(&AgentRole::Reviewer).unwrap();
        assert_eq!(reviewer, "\"reviewer\"");

        let critic = serde_json::to_string(&AgentRole::Critic).unwrap();
        assert_eq!(critic, "\"critic\"");

        let custom = serde_json::to_string(&AgentRole::Custom("specialist".to_string())).unwrap();
        let deserialized: AgentRole = serde_json::from_str(&custom).unwrap();
        assert_eq!(deserialized, AgentRole::Custom("specialist".to_string()));
    }

    #[test]
    fn agent_role_display_matches_serde() {
        assert_eq!(AgentRole::Planner.to_string(), "planner");
        assert_eq!(AgentRole::Executor.to_string(), "executor");
        assert_eq!(AgentRole::Reviewer.to_string(), "reviewer");
        assert_eq!(AgentRole::Critic.to_string(), "critic");
        assert_eq!(AgentRole::Custom("my-role".into()).to_string(), "my-role");
    }
}
