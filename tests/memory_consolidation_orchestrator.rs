use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use asteroniris::agent::loop_::run_main_session_turn_for_integration_with_policy;
use asteroniris::config::Config;
use asteroniris::memory::traits::MemoryLayer;
use asteroniris::memory::{
    run_consolidation_once, ConsolidationDisposition, ConsolidationInput, Memory, MemoryEventInput,
    MemoryEventType, MemorySource, PrivacyLevel, RecallQuery, SqliteMemory, CONSOLIDATION_SLOT_KEY,
};
use asteroniris::providers::Provider;
use asteroniris::security::policy::TenantPolicyContext;
use asteroniris::security::SecurityPolicy;
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;
use tempfile::TempDir;
use tokio::time::Instant;

struct FixedResponseProvider {
    response: String,
}

#[async_trait]
impl Provider for FixedResponseProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> Result<String> {
        Ok(self.response.clone())
    }
}

struct DelayedConsolidationMemory {
    inner: Arc<dyn Memory>,
    delay: Duration,
}

#[async_trait]
impl Memory for DelayedConsolidationMemory {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn health_check(&self) -> bool {
        self.inner.health_check().await
    }

    async fn append_event(
        &self,
        input: MemoryEventInput,
    ) -> anyhow::Result<asteroniris::memory::MemoryEvent> {
        if input.slot_key == CONSOLIDATION_SLOT_KEY {
            tokio::time::sleep(self.delay).await;
        }
        self.inner.append_event(input).await
    }

    async fn recall_scoped(
        &self,
        query: RecallQuery,
    ) -> anyhow::Result<Vec<asteroniris::memory::MemoryRecallItem>> {
        self.inner.recall_scoped(query).await
    }

    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<asteroniris::memory::BeliefSlot>> {
        self.inner.resolve_slot(entity_id, slot_key).await
    }

    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: asteroniris::memory::ForgetMode,
        reason: &str,
    ) -> anyhow::Result<asteroniris::memory::ForgetOutcome> {
        self.inner
            .forget_slot(entity_id, slot_key, mode, reason)
            .await
    }

    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        self.inner.count_events(entity_id).await
    }
}

#[tokio::test]
async fn memory_consolidation_is_idempotent() {
    let temp = TempDir::new().unwrap();
    let memory: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).unwrap());
    let entity_id = "tenant-alpha:user-1";

    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                "conversation.assistant_resp",
                MemoryEventType::FactAdded,
                "First response",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .unwrap();

    let checkpoint = memory.count_events(Some(entity_id)).await.unwrap();
    let input = ConsolidationInput::new(entity_id, checkpoint, "Question", "Answer");
    let before = memory.count_events(Some(entity_id)).await.unwrap();

    let first = run_consolidation_once(memory.as_ref(), temp.path(), &input)
        .await
        .unwrap();
    assert_eq!(first.disposition, ConsolidationDisposition::Consolidated);

    let after_first = memory.count_events(Some(entity_id)).await.unwrap();
    assert_eq!(after_first, before + 1);

    let second = run_consolidation_once(memory.as_ref(), temp.path(), &input)
        .await
        .unwrap();
    assert_eq!(
        second.disposition,
        ConsolidationDisposition::SkippedCheckpoint
    );

    let after_second = memory.count_events(Some(entity_id)).await.unwrap();
    assert_eq!(after_second, after_first);
}

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn memory_consolidation_runs_async_nonblocking() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = Config::default();
    config.workspace_dir = workspace.clone();
    config.memory.backend = "sqlite".to_string();
    config.memory.auto_save = true;
    config.persona.enabled_main_session = false;

    let base: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).unwrap());
    let delay = Duration::from_millis(700);
    let mem: Arc<dyn Memory> = Arc::new(DelayedConsolidationMemory {
        inner: base.clone(),
        delay,
    });

    let provider = FixedResponseProvider {
        response: "nonblocking consolidation response".to_string(),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    let entity_id = "tenant-alpha:user-42";

    let start = Instant::now();
    let response = run_main_session_turn_for_integration_with_policy(
        &config,
        &security,
        mem,
        &provider,
        &provider,
        "system",
        "test-model",
        0.3,
        entity_id,
        TenantPolicyContext::enabled("tenant-alpha"),
        "run turn quickly",
    )
    .await
    .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(response, "nonblocking consolidation response");
    assert!(
        elapsed < delay,
        "turn should complete before delayed consolidation task"
    );

    tokio::time::sleep(delay + Duration::from_millis(250)).await;
    let consolidated = base
        .resolve_slot(entity_id, CONSOLIDATION_SLOT_KEY)
        .await
        .unwrap();
    assert!(
        consolidated.is_some(),
        "async consolidation should complete"
    );
}

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn memory_consolidation_failure_isolated() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("state"), "blocked").unwrap();

    let mut config = Config::default();
    config.workspace_dir = workspace.clone();
    config.memory.backend = "sqlite".to_string();
    config.memory.auto_save = true;
    config.persona.enabled_main_session = false;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).unwrap());
    let provider = FixedResponseProvider {
        response: "response survives consolidation failure".to_string(),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    let entity_id = "tenant-alpha:user-99";

    let response = run_main_session_turn_for_integration_with_policy(
        &config,
        &security,
        mem.clone(),
        &provider,
        &provider,
        "system",
        "test-model",
        0.3,
        entity_id,
        TenantPolicyContext::enabled("tenant-alpha"),
        "keep answer path alive",
    )
    .await
    .unwrap();

    assert_eq!(response, "response survives consolidation failure");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let consolidated = mem
        .resolve_slot(entity_id, CONSOLIDATION_SLOT_KEY)
        .await
        .unwrap();
    assert!(
        consolidated.is_none(),
        "consolidation write should fail but turn response must succeed"
    );
}

#[tokio::test]
async fn memory_consolidation_long_run() {
    let temp = TempDir::new().expect("tempdir");
    let memory: Arc<dyn Memory> = Arc::new(SqliteMemory::new(temp.path()).expect("sqlite memory"));
    let entity_id = "tenant-alpha:long-run";
    let cycle_count = 40usize;

    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                "conversation.bootstrap",
                MemoryEventType::FactAdded,
                "bootstrap signal",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Working),
        )
        .await
        .expect("seed event append should succeed");

    let baseline_events = memory
        .count_events(Some(entity_id))
        .await
        .expect("baseline count");
    let mut expected_events = baseline_events;
    let mut expected_watermark = 0usize;

    for cycle in 0..cycle_count {
        let checkpoint = memory
            .count_events(Some(entity_id))
            .await
            .expect("checkpoint count");
        let input = ConsolidationInput::new(
            entity_id,
            checkpoint,
            format!("question cycle {cycle}"),
            format!("assistant cycle {cycle}"),
        );

        let first = run_consolidation_once(memory.as_ref(), temp.path(), &input)
            .await
            .expect("first consolidation should succeed");
        assert_eq!(first.disposition, ConsolidationDisposition::Consolidated);
        assert_eq!(first.previous_watermark, expected_watermark);
        assert_eq!(first.applied_watermark, checkpoint);

        expected_events += 1;
        expected_watermark = checkpoint;
        let after_first = memory
            .count_events(Some(entity_id))
            .await
            .expect("after first count");
        assert_eq!(after_first, expected_events);

        let second = run_consolidation_once(memory.as_ref(), temp.path(), &input)
            .await
            .expect("second consolidation should succeed");
        assert_eq!(
            second.disposition,
            ConsolidationDisposition::SkippedCheckpoint,
            "replaying same checkpoint must be idempotent"
        );
        assert_eq!(second.applied_watermark, expected_watermark);

        let after_second = memory
            .count_events(Some(entity_id))
            .await
            .expect("after second count");
        assert_eq!(after_second, expected_events);
    }

    let state_path = temp
        .path()
        .join("state")
        .join("memory_consolidation_state.json");
    let raw_state = std::fs::read_to_string(&state_path).expect("state file should exist");
    let parsed: Value = serde_json::from_str(&raw_state).expect("state file should be json");
    let watermark = parsed["watermarks"][entity_id]
        .as_u64()
        .expect("watermark should be a number") as usize;
    assert_eq!(watermark, expected_watermark);
    assert_eq!(
        memory
            .count_events(Some(entity_id))
            .await
            .expect("final count"),
        baseline_events + cycle_count,
        "long run should grow linearly with unique checkpoints only"
    );
}

#[tokio::test]
async fn memory_consolidation_long_run_decay_progression() {
    let temp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(temp.path()).expect("sqlite memory");
    let entity_id = "tenant-alpha:decay";
    let now = Utc::now();

    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                "decay.stale",
                MemoryEventType::FactAdded,
                "cache ttl fallback strategy with stale context",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.9)
            .with_layer(MemoryLayer::Semantic)
            .with_occurred_at((now - ChronoDuration::days(180)).to_rfc3339()),
        )
        .await
        .expect("append stale event");

    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                "decay.fresh",
                MemoryEventType::FactAdded,
                "cache ttl fallback strategy with fresh context",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.9)
            .with_layer(MemoryLayer::Semantic)
            .with_occurred_at(now.to_rfc3339()),
        )
        .await
        .expect("append fresh event");

    for checkpoint in 2..12 {
        let input = ConsolidationInput::new(
            entity_id,
            checkpoint,
            format!("decay-run question {checkpoint}"),
            format!("decay-run answer {checkpoint}"),
        );
        run_consolidation_once(&memory, temp.path(), &input)
            .await
            .expect("consolidation should succeed");
    }

    let first = memory
        .recall_scoped(RecallQuery::new(
            entity_id,
            "cache ttl fallback strategy",
            6,
        ))
        .await
        .expect("first recall should succeed");
    let second = memory
        .recall_scoped(RecallQuery::new(
            entity_id,
            "cache ttl fallback strategy",
            6,
        ))
        .await
        .expect("second recall should succeed");

    assert!(
        first.len() <= 6 && second.len() <= 6,
        "recall results must remain bounded by limit"
    );

    let fresh_idx = first
        .iter()
        .position(|item| item.slot_key == "decay.fresh")
        .expect("fresh slot should be present");
    let stale_idx = first
        .iter()
        .position(|item| item.slot_key == "decay.stale")
        .expect("stale slot should be present");
    assert!(
        fresh_idx < stale_idx,
        "fresh signal should outrank stale signal after long-run consolidation cycles"
    );

    let order_a: Vec<&str> = first.iter().map(|item| item.slot_key.as_str()).collect();
    let order_b: Vec<&str> = second.iter().map(|item| item.slot_key.as_str()).collect();
    assert_eq!(
        order_a, order_b,
        "decay ordering should remain deterministic"
    );
}
