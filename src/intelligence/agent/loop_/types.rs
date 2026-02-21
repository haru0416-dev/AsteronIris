use crate::intelligence::providers::Provider;
use crate::intelligence::tools::ToolRegistry;
use crate::security::PermissionStore;
use crate::security::policy::{EntityRateLimiter, TenantPolicyContext};
use anyhow::Result;
use std::sync::Arc;

pub(super) const PERSONA_PER_TURN_CALL_BUDGET: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TurnCallAccounting {
    pub(super) budget_limit: u8,
    pub(super) answer_calls: u8,
    pub(super) reflect_calls: u8,
}

impl TurnCallAccounting {
    pub(super) fn for_persona_mode(enabled: bool) -> Self {
        Self {
            budget_limit: if enabled {
                PERSONA_PER_TURN_CALL_BUDGET
            } else {
                1
            },
            answer_calls: 0,
            reflect_calls: 0,
        }
    }

    pub(super) fn total_calls(self) -> u8 {
        self.answer_calls + self.reflect_calls
    }

    pub(super) fn consume_answer_call(&mut self) -> Result<()> {
        self.answer_calls = self.answer_calls.saturating_add(1);
        self.ensure_budget()
    }

    pub(super) fn consume_reflect_call(&mut self) -> Result<()> {
        self.reflect_calls = self.reflect_calls.saturating_add(1);
        self.ensure_budget()
    }

    fn ensure_budget(self) -> Result<()> {
        if self.total_calls() > self.budget_limit {
            anyhow::bail!(
                "persona per-turn call budget exceeded: consumed={} budget={}",
                self.total_calls(),
                self.budget_limit
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TurnExecutionOutcome {
    pub(super) response: String,
    pub(super) tokens_used: Option<u64>,
    pub(super) accounting: TurnCallAccounting,
}

pub(super) struct MainSessionTurnParams<'a> {
    pub(super) answer_provider: &'a dyn Provider,
    pub(super) reflect_provider: &'a dyn Provider,
    pub(super) system_prompt: &'a str,
    pub(super) model_name: &'a str,
    pub(super) temperature: f64,
    pub(super) registry: Arc<ToolRegistry>,
    pub(super) max_tool_iterations: u32,
    pub(super) rate_limiter: Arc<EntityRateLimiter>,
    pub(super) permission_store: Arc<PermissionStore>,
}

#[derive(Debug, Clone)]
pub(in crate::intelligence::agent) struct RuntimeMemoryWriteContext {
    pub(in crate::intelligence::agent) entity_id: String,
    pub(in crate::intelligence::agent) policy_context: TenantPolicyContext,
}

impl RuntimeMemoryWriteContext {
    pub(super) fn main_session_default() -> Self {
        Self {
            entity_id: "default".to_string(),
            policy_context: TenantPolicyContext::disabled(),
        }
    }

    pub(super) fn for_entity_with_policy(
        entity_id: impl Into<String>,
        policy_context: TenantPolicyContext,
    ) -> Self {
        Self {
            entity_id: entity_id.into(),
            policy_context,
        }
    }

    pub(in crate::intelligence::agent) fn enforce_write_scope(&self) -> Result<()> {
        self.policy_context
            .enforce_recall_scope(&self.entity_id)
            .map_err(anyhow::Error::msg)
    }
}
