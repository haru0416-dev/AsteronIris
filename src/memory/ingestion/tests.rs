use super::*;
use crate::memory::SqliteMemory;
use tempfile::TempDir;

#[tokio::test]
async fn ingestion_pipeline_writes_valid_envelope() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(Arc::clone(&mem));

    let envelope = SignalEnvelope::new(
        SourceKind::Discord,
        "discord:12345",
        "hello from discord",
        "person:discord.12345",
    )
    .with_privacy_level(PrivacyLevel::Private)
    .with_signal_tier(SignalTier::Raw);

    let result = pipeline.ingest(envelope).await.unwrap();
    assert!(result.accepted);
    assert!(result.slot_key.starts_with("external.discord."));
}

#[tokio::test]
async fn ingestion_pipeline_rejects_empty_source_ref() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(mem);

    let envelope = SignalEnvelope::new(SourceKind::Api, "   ", "payload", "person:api.test");
    let err = pipeline.ingest(envelope).await.unwrap_err().to_string();
    assert!(err.contains("source_ref"));
}

#[tokio::test]
async fn ingestion_pipeline_rejects_source_ref_that_sanitizes_to_empty() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(mem);

    let envelope = SignalEnvelope::new(SourceKind::Api, "???", "payload", "person:api.test");
    let err = pipeline.ingest(envelope).await.unwrap_err().to_string();
    assert!(err.contains("signal_envelope.source_ref must not be empty"));
}

#[tokio::test]
async fn ingestion_pipeline_drops_exact_source_ref_duplicates() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(mem);

    let envelope = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-1",
        "stable payload",
        "person:api.test",
    );

    let first = pipeline.ingest(envelope.clone()).await.unwrap();
    assert!(first.accepted);

    let second = pipeline.ingest(envelope).await.unwrap();
    assert!(!second.accepted);
    assert_eq!(second.reason.as_deref(), Some("dedup:source_ref_exact"));
}

#[tokio::test]
async fn ingestion_pipeline_drops_semantically_similar_duplicates() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(mem);

    let first = SignalEnvelope::new(
        SourceKind::News,
        "news:item-1",
        "Asteroniris release planning update",
        "person:news.test",
    );
    let second = SignalEnvelope::new(
        SourceKind::News,
        "news:item-2",
        "Asteroniris release planning update",
        "person:news.test",
    );

    let first_result = pipeline.ingest(first).await.unwrap();
    assert!(first_result.accepted);

    let second_result = pipeline.ingest(second).await.unwrap();
    assert!(!second_result.accepted);
    assert_eq!(
        second_result.reason.as_deref(),
        Some("dedup:semantic_similar")
    );
}

#[tokio::test]
async fn ingestion_pipeline_keeps_same_content_when_source_kind_differs() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(mem);

    let api = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-same-content",
        "Asteroniris release planning update",
        "person:dedup.partition",
    );
    let news = SignalEnvelope::new(
        SourceKind::News,
        "news:item-same-content",
        "Asteroniris release planning update",
        "person:dedup.partition",
    );

    let api_result = pipeline.ingest(api).await.unwrap();
    assert!(api_result.accepted);

    let news_result = pipeline.ingest(news).await.unwrap();
    assert!(news_result.accepted);
    assert!(news_result.reason.is_none());
}

#[tokio::test]
async fn ingestion_pipeline_keeps_same_content_when_entity_id_differs() {
    let tmp = TempDir::new().unwrap();
    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).await.unwrap());
    let pipeline = SqliteIngestionPipeline::new(mem);

    let a = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-entity-a",
        "Asteroniris release planning update",
        "person:dedup.entity_a",
    );
    let b = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-entity-b",
        "Asteroniris release planning update",
        "person:dedup.entity_b",
    );

    let first = pipeline.ingest(a).await.unwrap();
    assert!(first.accepted);

    let second = pipeline.ingest(b).await.unwrap();
    assert!(second.accepted);
    assert!(second.reason.is_none());
}

#[test]
fn signal_envelope_normalize_sanitizes_fields() {
    let envelope = SignalEnvelope::new(
        SourceKind::Api,
        " api://user/123?x ",
        "  hello\n\tworld  ",
        " person:api user ",
    )
    .with_language(" JA_jp ");

    let normalized = envelope.normalize().unwrap();
    assert_eq!(normalized.source_ref, "api://user/123_x");
    assert_eq!(normalized.content, "hello world");
    assert_eq!(normalized.entity_id, "person:api_user");
    assert_eq!(normalized.language.as_deref(), Some("jajp"));
}

#[test]
fn signal_envelope_normalize_rejects_empty_content() {
    let envelope = SignalEnvelope::new(SourceKind::Api, "api:1", "   \n\t", "person:api.1");
    let err = envelope.normalize().unwrap_err().to_string();
    assert!(err.contains("content"));
}

#[test]
fn signal_envelope_classifies_risk_topic_and_entity_metadata() {
    let envelope = SignalEnvelope::new(
        SourceKind::News,
        "feed:item-1",
        "Unverified rumor about security policy leak with token",
        "person:alice",
    );

    let normalized = envelope.normalize().unwrap();
    let risk = normalized
        .metadata
        .get("risk_flags")
        .cloned()
        .unwrap_or_default();
    assert!(risk.contains("rumor"));
    assert!(risk.contains("unverified"));
    assert!(risk.contains("sensitive"));
    assert!(risk.contains("policy_risky"));
    assert_eq!(
        normalized.metadata.get("topic").map(String::as_str),
        Some("security")
    );
    assert_eq!(
        normalized.metadata.get("entity_hint").map(String::as_str),
        Some("person:alice")
    );
}

#[test]
fn signal_envelope_classification_preserves_preseeded_topic_and_entity_hint() {
    let envelope = SignalEnvelope::new(
        SourceKind::News,
        "feed:item-preserve",
        "release policy note",
        "person:bob",
    )
    .with_metadata("topic", "custom-topic")
    .with_metadata("entity_hint", "custom-entity");

    let normalized = envelope.normalize().unwrap();
    assert_eq!(
        normalized.metadata.get("topic").map(String::as_str),
        Some("custom-topic")
    );
    assert_eq!(
        normalized.metadata.get("entity_hint").map(String::as_str),
        Some("custom-entity")
    );
}

#[test]
fn signal_envelope_classification_emits_sorted_deduplicated_risk_flags() {
    let envelope = SignalEnvelope::new(
        SourceKind::News,
        "feed:item-risk",
        "rumor unverified leak policy secret guidance",
        "person:carol",
    );

    let normalized = envelope.normalize().unwrap();
    assert_eq!(
        normalized.metadata.get("risk_flags").map(String::as_str),
        Some("policy_risky|rumor|sensitive|unverified")
    );
}

#[test]
fn signal_envelope_classification_uses_source_kind_fallback_topic() {
    let envelope = SignalEnvelope::new(
        SourceKind::Manual,
        "trend:item-topic",
        "neutral payload without keyword hints",
        "person:trend.topic",
    );

    let normalized = envelope.normalize().unwrap();
    assert_eq!(
        normalized.metadata.get("topic").map(String::as_str),
        Some("manual")
    );
}

#[test]
fn signal_envelope_classification_uses_community_fallback_for_discord() {
    let envelope = SignalEnvelope::new(
        SourceKind::Discord,
        "discord:item-topic",
        "neutral payload without keyword hints",
        "person:discord.topic",
    );

    let normalized = envelope.normalize().unwrap();
    assert_eq!(
        normalized.metadata.get("topic").map(String::as_str),
        Some("community")
    );
}

#[test]
fn signal_envelope_normalize_rewrites_invalid_ingested_at() {
    let mut envelope = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-timestamp",
        "timestamp payload",
        "person:timestamp.test",
    );
    envelope.ingested_at = "not-a-rfc3339-timestamp".to_string();

    let normalized = envelope.normalize().unwrap();
    assert_ne!(normalized.ingested_at, "not-a-rfc3339-timestamp");
    assert!(chrono::DateTime::parse_from_rfc3339(&normalized.ingested_at).is_ok());
}

#[test]
fn signal_envelope_normalize_rejects_invalid_language_token() {
    let envelope = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-lang",
        "language payload",
        "person:language.test",
    )
    .with_language("###");

    let err = envelope.normalize().unwrap_err().to_string();
    assert!(err.contains("signal_envelope.language"));
}

#[test]
fn signal_envelope_normalize_rejects_entity_id_over_limit() {
    let long_entity = format!("person:{}", "a".repeat(140));
    let envelope = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-long-entity",
        "entity length payload",
        long_entity,
    );

    let err = envelope.normalize().unwrap_err().to_string();
    assert!(err.contains("signal_envelope.entity_id must be <= 128 chars"));
}

#[test]
fn signal_envelope_normalize_rejects_source_ref_over_limit() {
    let long_ref = format!("api:{}", "x".repeat(300));
    let envelope = SignalEnvelope::new(
        SourceKind::Api,
        long_ref,
        "source ref length payload",
        "person:source_ref.test",
    );

    let err = envelope.normalize().unwrap_err().to_string();
    assert!(err.contains("signal_envelope.source_ref must be <= 256 chars"));
}

#[test]
fn signal_envelope_json_roundtrip_preserves_required_fields() {
    let envelope = SignalEnvelope::new(
        SourceKind::Api,
        "api:item-json",
        "json payload",
        "person:json.test",
    )
    .with_signal_tier(SignalTier::Raw)
    .with_privacy_level(PrivacyLevel::Private)
    .with_language("en")
    .with_metadata("risk_flags", "sensitive");

    let json = serde_json::to_string(&envelope).expect("serialize envelope");
    let decoded: SignalEnvelope = serde_json::from_str(&json).expect("deserialize envelope");

    assert_eq!(decoded.source_kind, SourceKind::Api);
    assert_eq!(decoded.source_ref, "api:item-json");
    assert_eq!(decoded.entity_id, "person:json.test");
    assert_eq!(decoded.content, "json payload");
    assert_eq!(decoded.signal_tier, SignalTier::Raw);
    assert_eq!(decoded.privacy_level, PrivacyLevel::Private);
    assert_eq!(decoded.language.as_deref(), Some("en"));
    assert_eq!(
        decoded.metadata.get("risk_flags").map(String::as_str),
        Some("sensitive")
    );
}

#[test]
fn signal_envelope_json_rejects_invalid_source_kind() {
    let invalid = serde_json::json!({
        "source_kind": "unknown_kind",
        "source_ref": "api:item-json-invalid",
        "content": "json payload",
        "entity_id": "person:json.invalid",
        "signal_tier": "raw",
        "privacy_level": "private",
        "language": "en",
        "metadata": {},
        "ingested_at": chrono::Utc::now().to_rfc3339()
    });

    let err = serde_json::from_value::<SignalEnvelope>(invalid)
        .expect_err("invalid source_kind must fail");
    assert!(err.to_string().contains("unknown variant"));
}

#[test]
fn semantic_dedup_key_is_deterministic_and_source_scoped() {
    let key_a = semantic_dedup_key("person:key.test", SourceKind::Api, "same content");
    let key_b = semantic_dedup_key("person:key.test", SourceKind::Api, "same content");
    let key_other_source = semantic_dedup_key("person:key.test", SourceKind::News, "same content");
    let key_other_entity = semantic_dedup_key("person:key.other", SourceKind::Api, "same content");
    let key_other_content =
        semantic_dedup_key("person:key.test", SourceKind::Api, "different content");

    assert_eq!(key_a, key_b, "same tuple must produce stable dedup key");
    assert_ne!(
        key_a, key_other_source,
        "source_kind must partition dedup namespace"
    );
    assert_ne!(
        key_a, key_other_entity,
        "entity_id must partition dedup namespace"
    );
    assert_ne!(
        key_a, key_other_content,
        "content difference must change dedup key"
    );
}
