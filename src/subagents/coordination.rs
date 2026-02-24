use super::SubagentRunStatus;
use super::roles::{AgentRole, RoleAssignment, RoleConfig};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

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

pub struct CoordinationManager {
    sessions: HashMap<String, CoordinationSession>,
}

impl CoordinationManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, roles: Vec<RoleConfig>) -> Result<CoordinationSession> {
        let session_id = format!("coord_{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();

        let role_assignments = roles
            .into_iter()
            .map(|config| RoleAssignment {
                run_id: format!("run_{}", Uuid::new_v4()),
                role: config.role.clone(),
                config,
                assigned_at: now.clone(),
            })
            .collect();

        let session = CoordinationSession {
            session_id: session_id.clone(),
            roles: role_assignments,
            shared_context: SharedContext::default(),
            created_at: now,
        };

        self.sessions.insert(session_id, session.clone());
        Ok(session)
    }

    pub fn get_session(&self, session_id: &str) -> Option<&CoordinationSession> {
        self.sessions.get(session_id)
    }

    pub fn add_context_message(
        &mut self,
        session_id: &str,
        role: AgentRole,
        content: String,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(session_id) else {
            anyhow::bail!("session not found: {session_id}");
        };

        session.shared_context.messages.push(ContextMessage {
            role,
            content,
            timestamp: Utc::now().to_rfc3339(),
        });
        Ok(())
    }

    pub fn add_artifact(
        &mut self,
        session_id: &str,
        key: String,
        value: serde_json::Value,
    ) -> Result<()> {
        let Some(session) = self.sessions.get_mut(session_id) else {
            anyhow::bail!("session not found: {session_id}");
        };

        session.shared_context.artifacts.insert(key, value);
        Ok(())
    }

    pub fn get_shared_context(&self, session_id: &str) -> Option<&SharedContext> {
        self.sessions
            .get(session_id)
            .map(|session| &session.shared_context)
    }

    pub fn close_session(&mut self, session_id: &str) -> Result<()> {
        if self.sessions.remove(session_id).is_none() {
            anyhow::bail!("session not found: {session_id}");
        }
        Ok(())
    }
}

impl Default for CoordinationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lifecycle() {
        let mut manager = CoordinationManager::new();

        let roles = vec![
            RoleConfig {
                role: AgentRole::Planner,
                system_prompt_override: None,
                model_override: None,
                temperature_override: None,
                timeout_secs: None,
            },
            RoleConfig {
                role: AgentRole::Executor,
                system_prompt_override: None,
                model_override: None,
                temperature_override: None,
                timeout_secs: None,
            },
        ];
        let session = manager.create_session(roles).unwrap();
        let id = session.session_id.clone();

        assert!(manager.get_session(&id).is_some());
        assert!(id.starts_with("coord_"));
        assert_eq!(manager.get_session(&id).unwrap().roles.len(), 2);

        manager
            .add_context_message(&id, AgentRole::Planner, "Let's start".into())
            .unwrap();
        let ctx = manager.get_shared_context(&id).unwrap();
        assert_eq!(ctx.messages.len(), 1);
        assert_eq!(ctx.messages[0].content, "Let's start");

        manager
            .add_artifact(&id, "result".into(), serde_json::json!({ "value": 42 }))
            .unwrap();
        let ctx = manager.get_shared_context(&id).unwrap();
        assert!(ctx.artifacts.contains_key("result"));

        manager.close_session(&id).unwrap();
        assert!(manager.get_session(&id).is_none());
    }

    #[test]
    fn missing_session_returns_error() {
        let mut manager = CoordinationManager::new();

        let result =
            manager.add_context_message("nonexistent_id", AgentRole::Planner, "msg".into());
        assert!(result.is_err());

        let result = manager.close_session("nonexistent_id");
        assert!(result.is_err());

        let result = manager.add_artifact("nonexistent_id", "key".into(), serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn session_id_format() {
        let mut manager = CoordinationManager::new();
        let session = manager.create_session(vec![]).unwrap();
        assert!(
            session.session_id.starts_with("coord_"),
            "Session ID must start with 'coord_', got: {}",
            session.session_id
        );
    }

    #[test]
    fn session_lifecycle_three_roles_with_context() {
        let mut manager = CoordinationManager::new();

        let roles = vec![
            RoleConfig {
                role: AgentRole::Planner,
                system_prompt_override: None,
                model_override: None,
                temperature_override: None,
                timeout_secs: Some(30),
            },
            RoleConfig {
                role: AgentRole::Executor,
                system_prompt_override: None,
                model_override: None,
                temperature_override: None,
                timeout_secs: Some(60),
            },
            RoleConfig {
                role: AgentRole::Reviewer,
                system_prompt_override: None,
                model_override: None,
                temperature_override: None,
                timeout_secs: Some(10),
            },
        ];
        let session = manager.create_session(roles).unwrap();
        let id = session.session_id.clone();

        assert_eq!(manager.get_session(&id).unwrap().roles.len(), 3);

        manager
            .add_context_message(&id, AgentRole::Planner, "Here is the plan".into())
            .unwrap();
        manager
            .add_context_message(&id, AgentRole::Executor, "Executing step 1".into())
            .unwrap();
        manager
            .add_context_message(&id, AgentRole::Reviewer, "Reviewed step 1".into())
            .unwrap();

        let ctx = manager.get_shared_context(&id).unwrap();
        assert_eq!(ctx.messages.len(), 3);
        assert_eq!(ctx.messages[0].role, AgentRole::Planner);
        assert_eq!(ctx.messages[0].content, "Here is the plan");
        assert_eq!(ctx.messages[1].role, AgentRole::Executor);
        assert_eq!(ctx.messages[1].content, "Executing step 1");
        assert_eq!(ctx.messages[2].role, AgentRole::Reviewer);
        assert_eq!(ctx.messages[2].content, "Reviewed step 1");

        manager.close_session(&id).unwrap();
        assert!(manager.get_session(&id).is_none());
        assert!(manager.get_shared_context(&id).is_none());
    }
}
