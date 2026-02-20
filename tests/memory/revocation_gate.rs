use super::memory_harness;

use asteroniris::agent::loop_::build_context_for_integration;
use asteroniris::memory::{
    BeliefSlot, ForgetMode, ForgetOutcome, Memory, MemoryCategory, MemoryEvent, MemoryEventInput,
    MemoryRecallItem, MemorySource, PrivacyLevel, RecallQuery,
};
use asteroniris::security::policy::TenantPolicyContext;
use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{Connection, params};

fn insert_stale_belief_slot(conn: &Connection, entity_id: &str, slot_key: &str, value: &str) {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO belief_slots (
            entity_id, slot_key, value, status, winner_event_id,
            source, confidence, importance, privacy_level, updated_at
         ) VALUES (?1, ?2, ?3, 'active', 'stale-replay-winner', 'system', 1.0, 1.0, 'private', ?4)
         ON CONFLICT(entity_id, slot_key) DO UPDATE SET
            value = excluded.value,
            status = excluded.status,
            winner_event_id = excluded.winner_event_id,
            source = excluded.source,
            confidence = excluded.confidence,
            importance = excluded.importance,
            privacy_level = excluded.privacy_level,
            updated_at = excluded.updated_at",
        params![entity_id, slot_key, value, now],
    )
    .unwrap();
}

fn insert_stale_retrieval_doc(
    conn: &Connection,
    entity_id: &str,
    slot_key: &str,
    value: &str,
    provenance_source_class: Option<&str>,
    provenance_reference: Option<&str>,
) {
    let now = Utc::now().to_rfc3339();
    let doc_id = format!("{entity_id}:{slot_key}");
    conn.execute(
        "INSERT INTO retrieval_docs (
            doc_id, entity_id, slot_key, text_body, layer,
            provenance_source_class, provenance_reference, provenance_evidence_uri,
            retention_tier, retention_expires_at,
            recency_score, importance, reliability, contradiction_penalty, visibility, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'working', ?5, ?6, NULL, 'working', NULL, 1.0, 1.0, 1.0, 0.0, 'private', ?7)
         ON CONFLICT(doc_id) DO UPDATE SET
            text_body = excluded.text_body,
            layer = excluded.layer,
            provenance_source_class = excluded.provenance_source_class,
            provenance_reference = excluded.provenance_reference,
            provenance_evidence_uri = excluded.provenance_evidence_uri,
            retention_tier = excluded.retention_tier,
            retention_expires_at = excluded.retention_expires_at,
            recency_score = excluded.recency_score,
            importance = excluded.importance,
            reliability = excluded.reliability,
            contradiction_penalty = excluded.contradiction_penalty,
            visibility = excluded.visibility,
            updated_at = excluded.updated_at",
        params![
            doc_id,
            entity_id,
            slot_key,
            value,
            provenance_source_class,
            provenance_reference,
            now
        ],
    )
    .unwrap();
}

#[tokio::test]
async fn memory_revocation_gate_blocks_replay() {
    let (tmp, memory) = memory_harness::sqlite_fixture();
    let entity_id = "default";
    let slot_key = "profile.revoked_token";
    let revoked_value = "sk-revoked-123";

    memory_harness::append_test_event(
        &memory,
        entity_id,
        slot_key,
        revoked_value,
        MemoryCategory::Core,
    )
    .await;

    memory
        .forget_slot(
            entity_id,
            slot_key,
            ForgetMode::Tombstone,
            "revocation-test",
        )
        .await
        .unwrap();

    let db_path = tmp.path().join("memory").join("brain.db");
    let conn = Connection::open(db_path).unwrap();
    conn.execute(
        "DELETE FROM deletion_ledger WHERE entity_id = ?1 AND target_slot_key = ?2",
        params![entity_id, slot_key],
    )
    .unwrap();

    insert_stale_belief_slot(&conn, entity_id, slot_key, revoked_value);
    insert_stale_retrieval_doc(
        &conn,
        entity_id,
        slot_key,
        revoked_value,
        Some("system"),
        Some("lancedb:degraded:tombstone_marker_rewrite"),
    );

    let recalled = memory
        .recall_scoped(RecallQuery::new(entity_id, "revoked", 5))
        .await
        .unwrap();
    assert!(
        recalled.is_empty(),
        "revocation gate must block replay rows carrying revoked provenance markers"
    );

    let context = build_context_for_integration(
        &memory,
        entity_id,
        "revoked",
        TenantPolicyContext::disabled(),
    )
    .await
    .unwrap();
    assert!(!context.contains(revoked_value));
}

struct ReplayBypassMemory;

#[async_trait]
impl Memory for ReplayBypassMemory {
    fn name(&self) -> &str {
        "mock-replay-bypass"
    }

    async fn health_check(&self) -> bool {
        true
    }

    async fn append_event(&self, _input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
        anyhow::bail!("append_event not used")
    }

    async fn recall_scoped(&self, _query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
        Ok(vec![MemoryRecallItem {
            entity_id: "default".to_string(),
            slot_key: "profile.cached_secret".to_string(),
            value: "should-not-replay".to_string(),
            source: MemorySource::System,
            confidence: 0.9,
            importance: 0.9,
            privacy_level: PrivacyLevel::Private,
            score: 0.95,
            occurred_at: Utc::now().to_rfc3339(),
        }])
    }

    async fn resolve_slot(
        &self,
        _entity_id: &str,
        _slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        Ok(None)
    }

    async fn forget_slot(
        &self,
        _entity_id: &str,
        _slot_key: &str,
        _mode: ForgetMode,
        _reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        anyhow::bail!("forget_slot not used")
    }

    async fn count_events(&self, _entity_id: Option<&str>) -> anyhow::Result<usize> {
        Ok(0)
    }
}

#[tokio::test]
async fn memory_revocation_gate_applies_in_context_builder() {
    let mem = ReplayBypassMemory;
    let context = build_context_for_integration(
        &mem,
        "default",
        "cached_secret",
        TenantPolicyContext::disabled(),
    )
    .await
    .unwrap();

    assert!(
        context.is_empty(),
        "context builder must apply replay gate even when recall path is stale"
    );
}

#[tokio::test]
async fn memory_revocation_gate_blocks_cached_replay() {
    let (tmp, memory) = memory_harness::sqlite_fixture();
    let entity_id = "default";
    let slot_key = "profile.cached_replay";
    let stale_value = "stale-replay-value";

    memory_harness::append_test_event(
        &memory,
        entity_id,
        slot_key,
        stale_value,
        MemoryCategory::Core,
    )
    .await;
    memory
        .forget_slot(entity_id, slot_key, ForgetMode::Hard, "cache-replay-test")
        .await
        .unwrap();

    let db_path = tmp.path().join("memory").join("brain.db");
    let conn = Connection::open(db_path).unwrap();
    insert_stale_belief_slot(&conn, entity_id, slot_key, stale_value);
    insert_stale_retrieval_doc(
        &conn,
        entity_id,
        slot_key,
        stale_value,
        Some("explicit_user"),
        Some("agent.autosave.user_msg"),
    );

    let recalled = memory
        .recall_scoped(RecallQuery::new(entity_id, "stale-replay", 5))
        .await
        .unwrap();
    assert!(
        recalled.is_empty(),
        "denylist gate must block stale replay rows after hard delete"
    );
}
