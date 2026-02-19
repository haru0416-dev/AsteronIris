use super::memory_harness::append_test_event;
use super::memory_harness::sqlite_fixture;
use asteroniris::agent::loop_::build_context_for_integration;
use asteroniris::agent::loop_::run_main_session_turn_for_integration_with_policy;
use asteroniris::config::Config;
use asteroniris::memory::{Memory, MemoryCategory, RecallQuery};
use asteroniris::providers::Provider;
use asteroniris::security::policy::{
    TenantPolicyContext, TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR,
    TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR,
};
use asteroniris::security::SecurityPolicy;
use asteroniris::tools::memory_recall::MemoryRecallTool;
use asteroniris::tools::traits::Tool;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;

struct CaptureProvider {
    reply: String,
    last_message: Arc<Mutex<Option<String>>>,
}

impl CaptureProvider {
    fn new(reply: impl Into<String>) -> Self {
        Self {
            reply: reply.into(),
            last_message: Arc::new(Mutex::new(None)),
        }
    }

    fn captured_message(&self) -> Option<String> {
        self.last_message
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Provider for CaptureProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        *self
            .last_message
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(message.to_string());
        Ok(self.reply.clone())
    }
}

#[tokio::test]
async fn tenant_recall_blocks_cross_scope() {
    let (_tmp_dir, memory) = sqlite_fixture();

    append_test_event(
        &memory,
        "tenant-alpha:user-001",
        "profile.language",
        "Primary language is Rust",
        MemoryCategory::Core,
    )
    .await;
    append_test_event(
        &memory,
        "tenant-beta:user-001",
        "profile.language",
        "Primary language is Go",
        MemoryCategory::Core,
    )
    .await;

    let allowed = memory
        .recall_scoped(
            RecallQuery::new("tenant-alpha:user-001", "language", 5)
                .with_policy_context(TenantPolicyContext::enabled("tenant-alpha")),
        )
        .await
        .unwrap();
    assert_eq!(allowed.len(), 1, "same-tenant recall should succeed");
    assert_eq!(allowed[0].entity_id, "tenant-alpha:user-001");

    let err = memory
        .recall_scoped(
            RecallQuery::new("tenant-beta:user-001", "language", 5)
                .with_policy_context(TenantPolicyContext::enabled("tenant-alpha")),
        )
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR,
        "cross-tenant recall must be denied deterministically"
    );
}

#[tokio::test]
async fn tenant_mode_disables_default_fallback() {
    let (_tmp_dir, memory) = sqlite_fixture();

    append_test_event(
        &memory,
        "tenant-alpha:user-002",
        "profile.timezone",
        "Timezone is UTC",
        MemoryCategory::Core,
    )
    .await;

    let err = memory
        .recall_scoped(
            RecallQuery::new("default", "timezone", 5)
                .with_policy_context(TenantPolicyContext::enabled("tenant-alpha")),
        )
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR,
        "tenant mode must reject default-scope fallback"
    );
}

#[tokio::test]
async fn tenant_recall_all_entrypoints_allow_same_tenant() {
    let (_tmp_dir, memory) = sqlite_fixture();
    let memory: Arc<dyn Memory> = Arc::new(memory);

    append_test_event(
        memory.as_ref(),
        "tenant-alpha:user-003",
        "profile.language",
        "Primary language is Rust",
        MemoryCategory::Core,
    )
    .await;

    let policy_context = TenantPolicyContext::enabled("tenant-alpha");

    let direct = memory
        .recall_scoped(
            RecallQuery::new("tenant-alpha:user-003", "language", 5)
                .with_policy_context(policy_context.clone()),
        )
        .await
        .unwrap();
    assert_eq!(direct.len(), 1);

    let tool = MemoryRecallTool::new(memory.clone());
    let tool_result = tool
        .execute(json!({
            "entity_id": "tenant-alpha:user-003",
            "query": "language",
            "limit": 5,
            "policy_context": {
                "tenant_mode_enabled": true,
                "tenant_id": "tenant-alpha"
            }
        }))
        .await
        .unwrap();
    assert!(tool_result.success);
    assert!(tool_result.output.contains("Primary language is Rust"));

    let loop_context = build_context_for_integration(
        memory.as_ref(),
        "tenant-alpha:user-003",
        "language",
        policy_context,
    )
    .await;
    assert!(loop_context.is_ok());
}

#[tokio::test]
async fn tenant_recall_all_entrypoints_block_cross_scope() {
    let (_tmp_dir, memory) = sqlite_fixture();
    let memory: Arc<dyn Memory> = Arc::new(memory);

    append_test_event(
        memory.as_ref(),
        "tenant-beta:user-004",
        "profile.timezone",
        "Timezone is UTC",
        MemoryCategory::Core,
    )
    .await;

    let policy_context = TenantPolicyContext::enabled("tenant-alpha");

    let direct_err = memory
        .recall_scoped(
            RecallQuery::new("tenant-beta:user-004", "timezone", 5)
                .with_policy_context(policy_context.clone()),
        )
        .await
        .unwrap_err();
    assert_eq!(
        direct_err.to_string(),
        TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR
    );

    let tool = MemoryRecallTool::new(memory.clone());
    let tool_result = tool
        .execute(json!({
            "entity_id": "tenant-beta:user-004",
            "query": "timezone",
            "limit": 5,
            "policy_context": {
                "tenant_mode_enabled": true,
                "tenant_id": "tenant-alpha"
            }
        }))
        .await
        .unwrap();
    assert!(!tool_result.success);
    assert_eq!(
        tool_result.error,
        Some(format!(
            "Memory recall failed: {TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR}"
        ))
    );

    let loop_err = build_context_for_integration(
        memory.as_ref(),
        "tenant-beta:user-004",
        "timezone",
        policy_context,
    )
    .await
    .unwrap_err();
    assert_eq!(loop_err.to_string(), TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR);
}

#[tokio::test]
async fn tenant_recall_e2e_same_tenant_paths() {
    let (_tmp_dir, memory) = sqlite_fixture();
    let memory: Arc<dyn Memory> = Arc::new(memory);

    append_test_event(
        memory.as_ref(),
        "tenant-alpha:user-005",
        "profile.language",
        "Primary language is Rust",
        MemoryCategory::Core,
    )
    .await;

    let policy_context = TenantPolicyContext::enabled("tenant-alpha");

    let tool = MemoryRecallTool::new(memory.clone());
    let tool_result = tool
        .execute(json!({
            "entity_id": "tenant-alpha:user-005",
            "query": "language",
            "limit": 5,
            "policy_context": {
                "tenant_mode_enabled": true,
                "tenant_id": "tenant-alpha"
            }
        }))
        .await
        .unwrap();
    assert!(tool_result.success);
    assert!(tool_result.output.contains("Primary language is Rust"));

    let loop_context = build_context_for_integration(
        memory.as_ref(),
        "tenant-alpha:user-005",
        "language",
        policy_context.clone(),
    )
    .await
    .unwrap();
    assert!(loop_context.contains("Primary language is Rust"));

    let mut config = Config::default();
    config.memory.auto_save = false;
    config.persona.enabled_main_session = false;
    config.autonomy.verify_repair_max_attempts = 1;
    config.autonomy.verify_repair_max_repair_depth = 0;
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    let provider = CaptureProvider::new("ok");

    let answer = run_main_session_turn_for_integration_with_policy(
        &config,
        &security,
        memory,
        &provider,
        &provider,
        "system",
        "test-model",
        0.0,
        "tenant-alpha:user-005",
        policy_context,
        "language",
    )
    .await
    .unwrap();

    assert_eq!(answer, "ok");
    let captured = provider
        .captured_message()
        .expect("provider should capture enriched input");
    assert!(captured.contains("Primary language is Rust"));
}

#[tokio::test]
async fn tenant_recall_e2e_cross_tenant_block() {
    let (_tmp_dir, memory) = sqlite_fixture();
    let memory: Arc<dyn Memory> = Arc::new(memory);

    append_test_event(
        memory.as_ref(),
        "tenant-beta:user-006",
        "profile.timezone",
        "Timezone is UTC",
        MemoryCategory::Core,
    )
    .await;

    let tenant_alpha_context = TenantPolicyContext::enabled("tenant-alpha");

    let tool = MemoryRecallTool::new(memory.clone());
    let cross_tool_result = tool
        .execute(json!({
            "entity_id": "tenant-beta:user-006",
            "query": "timezone",
            "limit": 5,
            "policy_context": {
                "tenant_mode_enabled": true,
                "tenant_id": "tenant-alpha"
            }
        }))
        .await
        .unwrap();
    assert!(!cross_tool_result.success);
    assert_eq!(
        cross_tool_result.error,
        Some(format!(
            "Memory recall failed: {TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR}"
        ))
    );

    let default_tool_result = tool
        .execute(json!({
            "entity_id": "default",
            "query": "timezone",
            "limit": 5,
            "policy_context": {
                "tenant_mode_enabled": true,
                "tenant_id": "tenant-alpha"
            }
        }))
        .await
        .unwrap();
    assert!(!default_tool_result.success);
    assert_eq!(
        default_tool_result.error,
        Some(format!(
            "Memory recall failed: {TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR}"
        ))
    );

    let loop_cross_err = build_context_for_integration(
        memory.as_ref(),
        "tenant-beta:user-006",
        "timezone",
        tenant_alpha_context.clone(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        loop_cross_err.to_string(),
        TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR
    );

    let loop_default_err = build_context_for_integration(
        memory.as_ref(),
        "default",
        "timezone",
        tenant_alpha_context.clone(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        loop_default_err.to_string(),
        TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR
    );

    let mut config = Config::default();
    config.memory.auto_save = false;
    config.persona.enabled_main_session = false;
    config.autonomy.verify_repair_max_attempts = 1;
    config.autonomy.verify_repair_max_repair_depth = 0;
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    let provider = CaptureProvider::new("ok");

    let runtime_cross_err = run_main_session_turn_for_integration_with_policy(
        &config,
        &security,
        memory.clone(),
        &provider,
        &provider,
        "system",
        "test-model",
        0.0,
        "tenant-beta:user-006",
        tenant_alpha_context.clone(),
        "timezone",
    )
    .await
    .unwrap_err();
    assert_eq!(
        runtime_cross_err.to_string(),
        format!(
            "verify/repair escalated: reason=max_attempts_reached attempts=1 repair_depth=0 max_attempts=1 max_repair_depth=0 failure_class=transient_failure last_error={TENANT_RECALL_CROSS_SCOPE_DENIED_ERROR}"
        )
    );

    let runtime_default_err = run_main_session_turn_for_integration_with_policy(
        &config,
        &security,
        memory,
        &provider,
        &provider,
        "system",
        "test-model",
        0.0,
        "default",
        tenant_alpha_context,
        "timezone",
    )
    .await
    .unwrap_err();
    assert_eq!(
        runtime_default_err.to_string(),
        format!(
            "verify/repair escalated: reason=max_attempts_reached attempts=1 repair_depth=0 max_attempts=1 max_repair_depth=0 failure_class=transient_failure last_error={TENANT_DEFAULT_SCOPE_FALLBACK_DENIED_ERROR}"
        )
    );
}
