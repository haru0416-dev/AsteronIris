use std::sync::{Arc, Mutex};

use anyhow::Result;
use asteroniris::config::{Config, PersonaConfig};
use asteroniris::core::agent::loop_::{
    IntegrationTurnParams, run_main_session_turn_for_integration,
};
use asteroniris::core::memory::{Memory, SqliteMemory};
use asteroniris::core::persona::state_header::StateHeader;
use asteroniris::core::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use asteroniris::core::providers::Provider;
use asteroniris::platform::cron::{self, CronJobKind, CronJobOrigin};
use asteroniris::security::SecurityPolicy;
use asteroniris::security::policy::TenantPolicyContext;
use std::future::Future;
use std::pin::Pin;
use tempfile::TempDir;

struct SequenceProvider {
    responses: Mutex<Vec<Result<String>>>,
}

impl SequenceProvider {
    fn new(responses: Vec<Result<String>>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

impl Provider for SequenceProvider {
    fn chat_with_system<'a>(
        &'a self,
        _system_prompt: Option<&'a str>,
        _message: &'a str,
        _model: &'a str,
        _temperature: f64,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let mut responses = self
                .responses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if responses.is_empty() {
                return Ok("{}".to_string());
            }

            responses.remove(0)
        })
    }
}

#[allow(clippy::field_reassign_with_default)]
fn test_config(workspace_dir: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.workspace_dir = workspace_dir.to_path_buf();
    config.memory.backend = "sqlite".to_string();
    config.memory.auto_save = false;
    config.identity.person_id = Some("person-test".to_string());
    config.persona = PersonaConfig {
        enabled_main_session: true,
        ..PersonaConfig::default()
    };
    config
}

fn seeded_state() -> StateHeader {
    StateHeader {
        identity_principles_hash: "identity-v1-abcd1234".to_string(),
        safety_posture: "strict".to_string(),
        current_objective: "Close autonomy loop deterministically".to_string(),
        open_loops: vec!["route reflect output into bounded queue".to_string()],
        next_actions: vec!["run integration suite".to_string()],
        commitments: vec!["do not bypass policy guards".to_string()],
        recent_context_summary: "Task 16 cross-layer setup".to_string(),
        last_updated_at: "2026-02-17T12:00:00Z".to_string(),
    }
}

#[tokio::test]
async fn persona_reflect_self_task_flows_through_scheduler_planner_route() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let config = test_config(&workspace);

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).expect("sqlite memory"));
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        "person-test",
    );
    persistence
        .persist_backend_canonical_and_sync_mirror(&seeded_state())
        .await
        .expect("seed canonical state");

    let answer_provider = SequenceProvider::new(vec![Ok("bounded-autonomy-answer".to_string())]);
    let reflect_provider = SequenceProvider::new(vec![Ok(serde_json::json!({
        "state_header": {
            "identity_principles_hash": "identity-v1-abcd1234",
            "safety_posture": "strict",
            "current_objective": "Execute bounded autonomy flow",
            "open_loops": ["self-task queued"],
            "next_actions": ["verify bounded execution"],
            "commitments": ["preserve intent-only seams"],
            "recent_context_summary": "reflect stage produced deterministic update",
            "last_updated_at": "2026-02-17T13:00:00Z"
        },
        "memory_append": ["reflect writeback accepted"],
        "self_tasks": [
            {
                "title": "policy-governed self task",
                "instructions": "attempt bounded execution only",
                "expires_at": "2026-02-17T14:00:00Z"
            }
        ]
    })
    .to_string())]);
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = run_main_session_turn_for_integration(IntegrationTurnParams {
        config: &config,
        security: &security,
        mem,
        answer_provider: &answer_provider,
        reflect_provider: &reflect_provider,
        system_prompt: "system",
        model_name: "test-model",
        temperature: 0.4,
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        user_message: "run full bounded autonomy cycle",
    })
    .await
    .expect("main session turn");
    assert_eq!(response, "bounded-autonomy-answer");

    let queued = cron::list_jobs(&config).expect("queued jobs");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].job_kind, CronJobKind::Agent);
    assert_eq!(queued[0].origin, CronJobOrigin::Agent);
    assert_eq!(
        queued[0].max_attempts,
        config.autonomy.verify_repair_max_attempts
    );
    assert!(queued[0].command.starts_with("plan:"));

    let (executed, output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config, &security, &queued[0],
        )
        .await;
    assert!(executed, "{output}");
    assert!(output.contains("route=agent-planner"), "{output}");
    assert!(output.contains("success=true"), "{output}");
    assert!(output.contains("retry_limit_reached=false"), "{output}");
}

#[tokio::test]
async fn persona_reflect_self_task_enqueue_rejects_payload_above_pending_cap() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let config = test_config(&workspace);

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).expect("sqlite memory"));
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        "person-test",
    );
    persistence
        .persist_backend_canonical_and_sync_mirror(&seeded_state())
        .await
        .expect("seed canonical state");

    let self_tasks = (0..6)
        .map(|idx| {
            serde_json::json!({
                "title": format!("self-task-{idx}"),
                "instructions": "attempt bounded execution only",
                "expires_at": "2026-02-17T14:00:00Z"
            })
        })
        .collect::<Vec<_>>();

    let answer_provider = SequenceProvider::new(vec![Ok("bounded-autonomy-answer".to_string())]);
    let reflect_provider = SequenceProvider::new(vec![Ok(serde_json::json!({
        "state_header": {
            "identity_principles_hash": "identity-v1-abcd1234",
            "safety_posture": "strict",
            "current_objective": "Execute bounded autonomy flow",
            "open_loops": ["self-task queued"],
            "next_actions": ["verify bounded execution"],
            "commitments": ["preserve intent-only seams"],
            "recent_context_summary": "reflect stage produced deterministic update",
            "last_updated_at": "2026-02-17T13:00:00Z"
        },
        "memory_append": ["reflect writeback accepted"],
        "self_tasks": self_tasks
    })
    .to_string())]);
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = run_main_session_turn_for_integration(IntegrationTurnParams {
        config: &config,
        security: &security,
        mem,
        answer_provider: &answer_provider,
        reflect_provider: &reflect_provider,
        system_prompt: "system",
        model_name: "test-model",
        temperature: 0.4,
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        user_message: "run full bounded autonomy cycle",
    })
    .await
    .expect("main session turn");
    assert_eq!(response, "bounded-autonomy-answer");

    let queued = cron::list_jobs(&config).expect("queued jobs");
    assert!(queued.is_empty());
}

#[tokio::test]
async fn persona_reflect_rejects_top_level_source_identity_injection() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let config = test_config(&workspace);

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).expect("sqlite memory"));
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        "person-test",
    );
    persistence
        .persist_backend_canonical_and_sync_mirror(&seeded_state())
        .await
        .expect("seed canonical state");

    let answer_provider = SequenceProvider::new(vec![Ok("bounded-autonomy-answer".to_string())]);
    let reflect_provider = SequenceProvider::new(vec![Ok(serde_json::json!({
        "source_kind": "discord",
        "source_ref": "channel:discord:attack",
        "state_header": {
            "identity_principles_hash": "identity-v1-abcd1234",
            "safety_posture": "strict",
            "current_objective": "Attempt identity overwrite",
            "open_loops": ["self-task queued"],
            "next_actions": ["verify bounded execution"],
            "commitments": ["preserve intent-only seams"],
            "recent_context_summary": "inject top-level source identity",
            "last_updated_at": "2026-02-17T13:00:00Z"
        },
        "memory_append": ["reflect writeback accepted"],
        "self_tasks": [
            {
                "title": "malicious self task",
                "instructions": "attempt bounded execution only",
                "expires_at": "2026-02-17T14:00:00Z"
            }
        ]
    })
    .to_string())]);
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = run_main_session_turn_for_integration(IntegrationTurnParams {
        config: &config,
        security: &security,
        mem,
        answer_provider: &answer_provider,
        reflect_provider: &reflect_provider,
        system_prompt: "system",
        model_name: "test-model",
        temperature: 0.4,
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        user_message: "run full bounded autonomy cycle",
    })
    .await
    .expect("main session turn");
    assert_eq!(response, "bounded-autonomy-answer");

    let queued = cron::list_jobs(&config).expect("queued jobs");
    assert!(queued.is_empty());
}

#[tokio::test]
async fn persona_reflect_rejects_top_level_source_kind_only_injection() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let config = test_config(&workspace);

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).expect("sqlite memory"));
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        "person-test",
    );
    persistence
        .persist_backend_canonical_and_sync_mirror(&seeded_state())
        .await
        .expect("seed canonical state");

    let answer_provider = SequenceProvider::new(vec![Ok("bounded-autonomy-answer".to_string())]);
    let reflect_provider = SequenceProvider::new(vec![Ok(serde_json::json!({
        "source_kind": "slack",
        "state_header": {
            "identity_principles_hash": "identity-v1-abcd1234",
            "safety_posture": "strict",
            "current_objective": "Attempt source kind overwrite",
            "open_loops": ["self-task queued"],
            "next_actions": ["verify bounded execution"],
            "commitments": ["preserve intent-only seams"],
            "recent_context_summary": "inject top-level source kind",
            "last_updated_at": "2026-02-17T13:00:00Z"
        },
        "memory_append": ["reflect writeback accepted"],
        "self_tasks": [
            {
                "title": "malicious source-kind task",
                "instructions": "attempt bounded execution only",
                "expires_at": "2026-02-17T14:00:00Z"
            }
        ]
    })
    .to_string())]);
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = run_main_session_turn_for_integration(IntegrationTurnParams {
        config: &config,
        security: &security,
        mem,
        answer_provider: &answer_provider,
        reflect_provider: &reflect_provider,
        system_prompt: "system",
        model_name: "test-model",
        temperature: 0.4,
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        user_message: "run full bounded autonomy cycle",
    })
    .await
    .expect("main session turn");
    assert_eq!(response, "bounded-autonomy-answer");

    let queued = cron::list_jobs(&config).expect("queued jobs");
    assert!(queued.is_empty());
}

#[tokio::test]
async fn persona_reflect_rejects_top_level_source_ref_only_injection() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let config = test_config(&workspace);

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).expect("sqlite memory"));
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        "person-test",
    );
    persistence
        .persist_backend_canonical_and_sync_mirror(&seeded_state())
        .await
        .expect("seed canonical state");

    let answer_provider = SequenceProvider::new(vec![Ok("bounded-autonomy-answer".to_string())]);
    let reflect_provider = SequenceProvider::new(vec![Ok(serde_json::json!({
        "source_ref": "channel:discord:attack",
        "state_header": {
            "identity_principles_hash": "identity-v1-abcd1234",
            "safety_posture": "strict",
            "current_objective": "Attempt source ref overwrite",
            "open_loops": ["self-task queued"],
            "next_actions": ["verify bounded execution"],
            "commitments": ["preserve intent-only seams"],
            "recent_context_summary": "inject top-level source ref",
            "last_updated_at": "2026-02-17T13:00:00Z"
        },
        "memory_append": ["reflect writeback accepted"],
        "self_tasks": [
            {
                "title": "malicious source-ref task",
                "instructions": "attempt bounded execution only",
                "expires_at": "2026-02-17T14:00:00Z"
            }
        ]
    })
    .to_string())]);
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = run_main_session_turn_for_integration(IntegrationTurnParams {
        config: &config,
        security: &security,
        mem,
        answer_provider: &answer_provider,
        reflect_provider: &reflect_provider,
        system_prompt: "system",
        model_name: "test-model",
        temperature: 0.4,
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        user_message: "run full bounded autonomy cycle",
    })
    .await
    .expect("main session turn");
    assert_eq!(response, "bounded-autonomy-answer");

    let queued = cron::list_jobs(&config).expect("queued jobs");
    assert!(queued.is_empty());
}

#[tokio::test]
async fn persona_reflect_enqueues_bounded_self_tasks_within_pending_cap() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    let config = test_config(&workspace);

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).expect("sqlite memory"));
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
        "person-test",
    );
    persistence
        .persist_backend_canonical_and_sync_mirror(&seeded_state())
        .await
        .expect("seed canonical state");

    let self_tasks = (0..5)
        .map(|idx| {
            serde_json::json!({
                "title": format!("self-task-{idx}"),
                "instructions": "attempt bounded execution only",
                "expires_at": "2026-02-17T14:00:00Z"
            })
        })
        .collect::<Vec<_>>();

    let answer_provider = SequenceProvider::new(vec![Ok("bounded-autonomy-answer".to_string())]);
    let reflect_provider = SequenceProvider::new(vec![Ok(serde_json::json!({
        "state_header": {
            "identity_principles_hash": "identity-v1-abcd1234",
            "safety_posture": "strict",
            "current_objective": "Execute bounded autonomy flow",
            "open_loops": ["self-task queued"],
            "next_actions": ["verify bounded execution"],
            "commitments": ["preserve intent-only seams"],
            "recent_context_summary": "reflect stage produced deterministic update",
            "last_updated_at": "2026-02-17T13:00:00Z"
        },
        "memory_append": ["reflect writeback accepted"],
        "self_tasks": self_tasks
    })
    .to_string())]);
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = run_main_session_turn_for_integration(IntegrationTurnParams {
        config: &config,
        security: &security,
        mem,
        answer_provider: &answer_provider,
        reflect_provider: &reflect_provider,
        system_prompt: "system",
        model_name: "test-model",
        temperature: 0.4,
        entity_id: "default",
        policy_context: TenantPolicyContext::disabled(),
        user_message: "run full bounded autonomy cycle",
    })
    .await
    .expect("main session turn");
    assert_eq!(response, "bounded-autonomy-answer");

    let queued = cron::list_jobs(&config).expect("queued jobs");
    assert!(!queued.is_empty());
    assert!(queued.len() <= 5);
    assert!(queued.iter().all(|job| job.origin == CronJobOrigin::Agent));
    assert!(
        queued
            .iter()
            .all(|job| job.max_attempts == config.autonomy.verify_repair_max_attempts)
    );
    assert!(queued.iter().all(|job| job.command.starts_with("plan:")));
}
