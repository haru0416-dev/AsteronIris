use super::*;
use crate::config::PersonaConfig;
use crate::memory::SqliteMemory;
use crate::persona::state_header::StateHeaderV1;
use crate::persona::state_persistence::BackendCanonicalStateHeaderPersistence;
use crate::providers::reliable::ReliableProvider;
use crate::security::SecurityPolicy;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use verify_repair::VERIFY_REPAIR_ESCALATION_SLOT_KEY;

struct MockProvider {
    calls: Arc<AtomicUsize>,
    responses: Vec<String>,
    fail_on_call: Option<usize>,
}

#[async_trait]
impl Provider for MockProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        let call_number = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_on_call == Some(call_number) {
            anyhow::bail!("mock failure on call {call_number}");
        }

        Ok(self
            .responses
            .get(call_number - 1)
            .cloned()
            .unwrap_or_else(|| "{}".to_string()))
    }
}

fn sample_state() -> StateHeaderV1 {
    StateHeaderV1 {
        schema_version: 1,
        identity_principles_hash: "identity-v1-abcd1234".to_string(),
        safety_posture: "strict".to_string(),
        current_objective: "Ship two-call main-session loop".to_string(),
        open_loops: vec!["Wire persona reflect stage".to_string()],
        next_actions: vec!["Add strict payload parsing".to_string()],
        commitments: vec!["Preserve answer path on call-2 failure".to_string()],
        recent_context_summary: "Task 4 integrates answer + reflect/writeback calls.".to_string(),
        last_updated_at: Utc::now().to_rfc3339(),
    }
}

fn build_reflect_payload(previous: &StateHeaderV1) -> String {
    json!({
        "state_header": {
            "schema_version": previous.schema_version,
            "identity_principles_hash": previous.identity_principles_hash,
            "safety_posture": previous.safety_posture,
            "current_objective": "Confirm two provider calls per turn",
            "open_loops": ["Validate call count invariant"],
            "next_actions": ["Run targeted persona loop tests"],
            "commitments": ["Keep main-session scope only"],
            "recent_context_summary": "Call-2 writes guarded payload with strict JSON parsing.",
            "last_updated_at": "2026-02-16T11:00:00Z"
        },
        "memory_append": ["persona loop writeback accepted"]
    })
    .to_string()
}

#[allow(clippy::field_reassign_with_default)]
fn test_config(workspace_dir: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.workspace_dir = workspace_dir.to_path_buf();
    config.memory.auto_save = false;
    config.persona = PersonaConfig {
        enabled_main_session: true,
        ..PersonaConfig::default()
    };
    config
}

fn noop_observer() -> Arc<dyn Observer> {
    Arc::new(NoopObserver)
}

fn main_turn_params<'a>(
    config: &Config,
    answer_provider: &'a dyn Provider,
    reflect_provider: &'a dyn Provider,
    system_prompt: &'a str,
    model_name: &'a str,
    temperature: f64,
) -> MainSessionTurnParams<'a> {
    MainSessionTurnParams {
        answer_provider,
        reflect_provider,
        system_prompt,
        model_name,
        temperature,
        registry: Arc::new(crate::tools::ToolRegistry::new(vec![])),
        max_tool_iterations: config.autonomy.max_tool_loop_iterations,
        rate_limiter: Arc::new(crate::security::EntityRateLimiter::new(
            config.autonomy.max_actions_per_hour,
            config.autonomy.max_actions_per_entity_per_hour,
        )),
        permission_store: Arc::new(crate::security::PermissionStore::load(
            &config.workspace_dir,
        )),
    }
}

#[tokio::test]
async fn persona_loop_two_calls_per_turn() {
    let temp = TempDir::new().unwrap();
    let config = test_config(temp.path());

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
    );
    let initial = sample_state();
    persistence
        .persist_backend_canonical_and_sync_mirror(&initial)
        .await
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MockProvider {
        calls: calls.clone(),
        responses: vec![
            "answer-call-output".to_string(),
            build_reflect_payload(&initial),
        ],
        fail_on_call: None,
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = execute_main_session_turn(
        &config,
        &security,
        mem.clone(),
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.4),
        "How do we wire Task 4?",
        &noop_observer(),
    )
    .await
    .unwrap();

    assert_eq!(response, "answer-call-output");
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let updated = persistence.load_backend_canonical().await.unwrap().unwrap();
    assert_eq!(
        updated.current_objective,
        "Confirm two provider calls per turn"
    );
}

#[tokio::test]
async fn persona_loop_call2_failure_preserves_answer() {
    let temp = TempDir::new().unwrap();
    let config = test_config(temp.path());

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
    );
    let initial = sample_state();
    persistence
        .persist_backend_canonical_and_sync_mirror(&initial)
        .await
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MockProvider {
        calls: calls.clone(),
        responses: vec!["answer-survives-call2-failure".to_string()],
        fail_on_call: Some(2),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = execute_main_session_turn(
        &config,
        &security,
        mem.clone(),
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.4),
        "Keep answer path stable",
        &noop_observer(),
    )
    .await
    .unwrap();

    assert_eq!(response, "answer-survives-call2-failure");
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let persisted = persistence.load_backend_canonical().await.unwrap().unwrap();
    assert_eq!(persisted, initial);
}

struct AlwaysFailProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Provider for AlwaysFailProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("transient reflect failure")
    }
}

struct TemperatureCaptureProvider {
    temperatures: Arc<Mutex<Vec<f64>>>,
    response: String,
}

struct MessageFailProvider {
    calls: Arc<AtomicUsize>,
    message: &'static str,
}

#[async_trait]
impl Provider for TemperatureCaptureProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.temperatures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(temperature);
        Ok(self.response.clone())
    }
}

#[async_trait]
impl Provider for MessageFailProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> anyhow::Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!(self.message)
    }
}

#[tokio::test]
async fn persona_reflect_no_retry() {
    let temp = TempDir::new().unwrap();
    let config = test_config(temp.path());

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
    );
    let initial = sample_state();
    persistence
        .persist_backend_canonical_and_sync_mirror(&initial)
        .await
        .unwrap();

    let answer_calls = Arc::new(AtomicUsize::new(0));
    let answer_provider = ReliableProvider::new(
        vec![(
            "primary".to_string(),
            Box::new(MockProvider {
                calls: answer_calls.clone(),
                responses: vec![
                    "unused-first-attempt".to_string(),
                    "answer-with-reliable-configured".to_string(),
                ],
                fail_on_call: Some(1),
            }),
        )],
        3,
        1,
    );

    let reflect_calls = Arc::new(AtomicUsize::new(0));
    let reflect_provider = AlwaysFailProvider {
        calls: reflect_calls.clone(),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = execute_main_session_turn(
        &config,
        &security,
        mem.clone(),
        &main_turn_params(
            &config,
            &answer_provider,
            &reflect_provider,
            "system",
            "test-model",
            0.2,
        ),
        "verify reflect retry suppression",
        &noop_observer(),
    )
    .await
    .unwrap();

    assert_eq!(response, "answer-with-reliable-configured");
    assert_eq!(answer_calls.load(Ordering::SeqCst), 2);
    assert_eq!(reflect_calls.load(Ordering::SeqCst), 1);

    let persisted = persistence.load_backend_canonical().await.unwrap().unwrap();
    assert_eq!(persisted, initial);
}

#[tokio::test]
async fn persona_budget_counter_stable() {
    let temp = TempDir::new().unwrap();
    let config = test_config(temp.path());

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let persistence = BackendCanonicalStateHeaderPersistence::new(
        mem.clone(),
        config.workspace_dir.clone(),
        config.persona.clone(),
    );
    let initial = sample_state();
    persistence
        .persist_backend_canonical_and_sync_mirror(&initial)
        .await
        .unwrap();

    let answer_calls = Arc::new(AtomicUsize::new(0));
    let answer_provider = MockProvider {
        calls: answer_calls.clone(),
        responses: vec![
            "turn-1-answer".to_string(),
            "turn-2-answer".to_string(),
            "turn-3-answer".to_string(),
        ],
        fail_on_call: None,
    };

    let reflect_calls = Arc::new(AtomicUsize::new(0));
    let reflect_provider = ReliableProvider::new(
        vec![(
            "primary".to_string(),
            Box::new(MockProvider {
                calls: reflect_calls.clone(),
                responses: vec![
                    build_reflect_payload(&initial),
                    build_reflect_payload(&initial),
                    build_reflect_payload(&initial),
                ],
                fail_on_call: None,
            }),
        )],
        3,
        1,
    );
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    for turn in 0..3 {
        let outcome = execute_main_session_turn_with_accounting(
            &config,
            &security,
            mem.clone(),
            &main_turn_params(
                &config,
                &answer_provider,
                &reflect_provider,
                "system",
                "test-model",
                0.2,
            ),
            &format!("turn-{turn}-message"),
            &RuntimeMemoryWriteContext::main_session_default(),
            &noop_observer(),
        )
        .await
        .unwrap();

        assert_eq!(
            outcome.accounting.budget_limit,
            PERSONA_PER_TURN_CALL_BUDGET
        );
        assert_eq!(outcome.accounting.answer_calls, 1);
        assert_eq!(outcome.accounting.reflect_calls, 1);
        assert_eq!(outcome.accounting.total_calls(), 2);
    }

    assert_eq!(answer_calls.load(Ordering::SeqCst), 3);
    assert_eq!(reflect_calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn persona_loop_policy_blocks_when_action_limit_is_exhausted() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.autonomy.max_actions_per_hour = 0;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MockProvider {
        calls: calls.clone(),
        responses: vec!["should-not-run".to_string()],
        fail_on_call: None,
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let err = execute_main_session_turn(
        &config,
        &security,
        mem,
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.1),
        "blocked by policy",
        &noop_observer(),
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("action limit exceeded"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn autonomy_temperature_clamped() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.persona.enabled_main_session = false;
    config.autonomy.level = crate::security::AutonomyLevel::Full;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let temperatures = Arc::new(Mutex::new(Vec::new()));
    let provider = TemperatureCaptureProvider {
        temperatures: temperatures.clone(),
        response: "clamped-temp-response".to_string(),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = execute_main_session_turn(
        &config,
        &security,
        mem,
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 1.9),
        "clamp this temperature",
        &noop_observer(),
    )
    .await
    .unwrap();

    assert_eq!(response, "clamped-temp-response");
    let seen = temperatures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(seen, vec![1.0]);
}

#[tokio::test]
async fn verify_repair_recovers_within_attempt_cap() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.persona.enabled_main_session = false;
    config.autonomy.verify_repair_max_attempts = 3;
    config.autonomy.verify_repair_max_repair_depth = 2;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MockProvider {
        calls: calls.clone(),
        responses: vec![
            "unused-first-attempt".to_string(),
            "recovered-on-second-attempt".to_string(),
        ],
        fail_on_call: Some(1),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = execute_main_session_turn(
        &config,
        &security,
        mem,
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.2),
        "recover after one failure",
        &noop_observer(),
    )
    .await
    .unwrap();

    assert_eq!(response, "recovered-on-second-attempt");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn verify_repair_stops_at_max_attempts() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.persona.enabled_main_session = false;
    config.autonomy.verify_repair_max_attempts = 3;
    config.autonomy.verify_repair_max_repair_depth = 2;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MessageFailProvider {
        calls: calls.clone(),
        message: "deterministic transient failure",
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let err = execute_main_session_turn(
        &config,
        &security,
        mem,
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.2),
        "always fail",
        &noop_observer(),
    )
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("reason=max_attempts_reached"));
    assert!(message.contains("attempts=3"));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn verify_repair_emits_escalation_event() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.persona.enabled_main_session = false;
    config.autonomy.verify_repair_max_attempts = 2;
    config.autonomy.verify_repair_max_repair_depth = 1;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MessageFailProvider {
        calls: calls.clone(),
        message: "deterministic retry failure",
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let err = execute_main_session_turn(
        &config,
        &security,
        mem.clone(),
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.2),
        "escalate and emit event",
        &noop_observer(),
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("reason=max_attempts_reached"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    let escalation = mem
        .resolve_slot("default", VERIFY_REPAIR_ESCALATION_SLOT_KEY)
        .await
        .unwrap()
        .expect("escalation event should be written");

    assert!(
        escalation
            .value
            .contains("\"reason\":\"max_attempts_reached\"")
    );
    assert!(escalation.value.contains("\"attempts\":2"));
    assert!(
        escalation
            .value
            .contains("\"failure_class\":\"transient_failure\"")
    );
}

#[tokio::test]
async fn verify_repair_retries_still_enforce_policy_limits() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.persona.enabled_main_session = false;
    config.autonomy.max_actions_per_hour = 2;
    config.autonomy.verify_repair_max_attempts = 5;
    config.autonomy.verify_repair_max_repair_depth = 4;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = MessageFailProvider {
        calls: calls.clone(),
        message: "retry until policy blocks",
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let err = execute_main_session_turn(
        &config,
        &security,
        mem,
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.2),
        "policy must gate every retry",
        &noop_observer(),
    )
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(message.contains("reason=non_retryable_failure"));
    assert!(message.contains("failure_class=policy_limit"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn post_turn_inference_hook_appends_tagged_events() {
    let temp = TempDir::new().unwrap();
    let mut config = test_config(temp.path());
    config.memory.auto_save = true;
    config.persona.enabled_main_session = false;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let provider = MockProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        responses: vec![
            "INFERRED_CLAIM inference.preference.language => User prefers Rust\nCONTRADICTION_EVENT contradiction.preference.language => Earlier note said Python".to_string(),
        ],
        fail_on_call: None,
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let response = execute_main_session_turn(
        &config,
        &security,
        mem.clone(),
        &main_turn_params(&config, &provider, &provider, "system", "test-model", 0.3),
        "derive inferences",
        &noop_observer(),
    )
    .await
    .unwrap();
    assert!(response.contains("INFERRED_CLAIM"));

    let inferred = mem
        .resolve_slot("default", "inference.preference.language")
        .await
        .unwrap()
        .expect("inferred claim should persist");
    assert_eq!(inferred.source, MemorySource::Inferred);

    let contradiction = mem
        .resolve_slot("default", "contradiction.preference.language")
        .await
        .unwrap()
        .expect("contradiction event should be represented as event");
    assert_eq!(contradiction.source, MemorySource::System);
}
