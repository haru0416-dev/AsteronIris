use asteroniris::core::memory::{
    IngestionPipeline, Memory, MemoryEventInput, MemoryEventType, MemoryInferenceEvent,
    MemorySource, PrivacyLevel, RecallQuery, SignalEnvelope, SignalTier, SourceKind,
    SqliteIngestionPipeline, SqliteMemory,
};
use chrono::{Duration, Utc};
use std::sync::Arc;
use tempfile::TempDir;

fn promotion_status_for(tmp: &TempDir, unit_id: &str) -> Option<String> {
    let db_path = tmp.path().join("memory").join("brain.db");
    let conn = rusqlite::Connection::open(db_path).expect("open sqlite db");
    conn.query_row(
        "SELECT promotion_status FROM retrieval_units WHERE unit_id = ?1",
        rusqlite::params![unit_id],
        |row| row.get(0),
    )
    .ok()
}

#[tokio::test]
async fn sqlite_retrieval_salience_decay_ordering() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    let now = Utc::now();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.preference.language",
                MemoryEventType::FactAdded,
                "Current preference: Rust language for backend work",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.96)
            .with_importance(0.95)
            .with_occurred_at(now.to_rfc3339()),
        )
        .await
        .expect("fresh high-salience insert");

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.preference.legacy",
                MemoryEventType::FactAdded,
                "Legacy preference: Rust language for backend work",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_confidence(0.25)
            .with_importance(0.20)
            .with_occurred_at((now - Duration::days(140)).to_rfc3339()),
        )
        .await
        .expect("stale low-salience insert");

    let recalled = memory
        .recall_scoped(RecallQuery::new("default", "Rust language", 10))
        .await
        .expect("recall succeeds");

    let fresh_idx = recalled
        .iter()
        .position(|item| item.slot_key == "profile.preference.language")
        .expect("fresh memory present");
    let stale_idx = recalled
        .iter()
        .position(|item| item.slot_key == "profile.preference.legacy")
        .expect("stale memory present");

    assert!(
        fresh_idx < stale_idx,
        "fresh high-salience memory must outrank stale low-salience memory"
    );
}

#[tokio::test]
async fn sqlite_contradiction_penalty_affects_order() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    let now = Utc::now();

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone.conflicted",
                MemoryEventType::FactAdded,
                "Timezone for meetings is UTC",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.95)
            .with_occurred_at(now.to_rfc3339()),
        )
        .await
        .expect("conflicted candidate insert");

    memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone.clean",
                MemoryEventType::FactAdded,
                "Timezone for meetings is UTC",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.90)
            .with_importance(0.90)
            .with_occurred_at((now - Duration::minutes(2)).to_rfc3339()),
        )
        .await
        .expect("clean baseline insert");

    memory
        .append_inference_event(
            MemoryInferenceEvent::contradiction_marked(
                "default",
                "profile.timezone.conflicted",
                "Detected contradiction against newer schedule metadata",
            )
            .with_confidence(1.0)
            .with_importance(1.0)
            .with_occurred_at((now + Duration::minutes(1)).to_rfc3339()),
        )
        .await
        .expect("contradiction event insert");

    let recalled_first = memory
        .recall_scoped(RecallQuery::new("default", "Timezone for meetings", 10))
        .await
        .expect("first recall succeeds");
    let recalled_second = memory
        .recall_scoped(RecallQuery::new("default", "Timezone for meetings", 10))
        .await
        .expect("second recall succeeds");

    let clean_idx = recalled_first
        .iter()
        .position(|item| item.slot_key == "profile.timezone.clean")
        .expect("clean slot present");
    let conflicted_idx = recalled_first
        .iter()
        .position(|item| item.slot_key == "profile.timezone.conflicted")
        .expect("conflicted slot present");

    assert!(
        clean_idx < conflicted_idx,
        "contradiction penalty must demote conflicted memory"
    );

    let order_first: Vec<&str> = recalled_first
        .iter()
        .map(|item| item.slot_key.as_str())
        .collect();
    let order_second: Vec<&str> = recalled_second
        .iter()
        .map(|item| item.slot_key.as_str())
        .collect();
    assert_eq!(order_first, order_second, "ordering must be deterministic");
}

#[tokio::test]
async fn sqlite_keyword_tokenization_or_matching() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");

    // Store three facts with distinct keyword combinations
    memory
        .append_event(
            MemoryEventInput::new(
                "user-keyword",
                "fact.rust_mozilla",
                MemoryEventType::FactAdded,
                "Rust programming language was created by Graydon Hoare at Mozilla",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.8),
        )
        .await
        .expect("insert rust fact");

    memory
        .append_event(
            MemoryEventInput::new(
                "user-keyword",
                "fact.tokyo_weather",
                MemoryEventType::FactAdded,
                "Tokyo weather in February is cold with average temperature around 6 degrees",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.8),
        )
        .await
        .expect("insert tokyo fact");

    memory
        .append_event(
            MemoryEventInput::new(
                "user-keyword",
                "fact.sushi_ginza",
                MemoryEventType::FactAdded,
                "The best sushi restaurant in Ginza serves omakase for 30000 yen",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.8),
        )
        .await
        .expect("insert sushi fact");

    // Test 1: Multi-keyword OR - both words from same fact
    let recalled = memory
        .recall_scoped(RecallQuery::new("user-keyword", "Graydon Mozilla", 10))
        .await
        .expect("recall Graydon Mozilla");
    assert!(
        recalled
            .iter()
            .any(|item| item.slot_key == "fact.rust_mozilla"),
        "multi-keyword OR should match Rust fact via Graydon OR Mozilla"
    );

    // Test 2: Single keyword matching
    let recalled = memory
        .recall_scoped(RecallQuery::new("user-keyword", "sushi", 10))
        .await
        .expect("recall sushi");
    assert!(
        recalled
            .iter()
            .any(|item| item.slot_key == "fact.sushi_ginza"),
        "single keyword should match sushi fact"
    );

    // Test 3: Cross-fact keyword - one word matches each different fact
    let recalled = memory
        .recall_scoped(RecallQuery::new("user-keyword", "Tokyo Ginza", 10))
        .await
        .expect("recall Tokyo Ginza");
    let keys: Vec<&str> = recalled.iter().map(|item| item.slot_key.as_str()).collect();
    assert!(
        keys.contains(&"fact.tokyo_weather"),
        "OR logic should match Tokyo weather fact via Tokyo: got {keys:?}"
    );
    assert!(
        keys.contains(&"fact.sushi_ginza"),
        "OR logic should match sushi fact via Ginza: got {keys:?}"
    );

    // Test 4: No match
    let recalled = memory
        .recall_scoped(RecallQuery::new("user-keyword", "Python Django", 10))
        .await
        .expect("recall Python Django");
    assert!(
        recalled.is_empty(),
        "unrelated keywords should return no results"
    );

    // Test 5: Two-keyword match scores higher than single-keyword match
    let recalled = memory
        .recall_scoped(RecallQuery::new("user-keyword", "Rust Mozilla", 10))
        .await
        .expect("recall Rust Mozilla");
    let rust_item = recalled
        .iter()
        .find(|item| item.slot_key == "fact.rust_mozilla")
        .expect("Rust fact must appear for Rust Mozilla");
    assert!(
        rust_item.score > 0.5,
        "dual-keyword match should have score > 0.5, got {}",
        rust_item.score
    );
}

#[tokio::test]
async fn sqlite_recall_edge_cases() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");

    memory
        .append_event(
            MemoryEventInput::new(
                "edge-entity",
                "edge.unicode",
                MemoryEventType::FactAdded,
                "User prefers Japanese locale with kanji characters",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.8),
        )
        .await
        .expect("insert unicode fact");

    // Edge 1: Empty query returns empty
    let recalled = memory
        .recall_scoped(RecallQuery::new("edge-entity", "", 10))
        .await
        .expect("empty query");
    assert!(recalled.is_empty(), "empty query should return nothing");

    // Edge 2: Whitespace-only query returns empty
    let recalled = memory
        .recall_scoped(RecallQuery::new("edge-entity", "   ", 10))
        .await
        .expect("whitespace query");
    assert!(
        recalled.is_empty(),
        "whitespace-only query should return nothing"
    );

    // Edge 3: Single-char keywords are filtered (len < 2), falls back to full query
    let recalled = memory
        .recall_scoped(RecallQuery::new("edge-entity", "a b c", 10))
        .await
        .expect("single-char keywords");
    // All keywords are < 2 chars, so falls back to full query match '%a b c%'
    assert!(
        recalled.is_empty(),
        "single-char-only keywords should not match unrelated content"
    );

    // Edge 4: Mixed short and long keywords - short ones filtered out
    let recalled = memory
        .recall_scoped(RecallQuery::new("edge-entity", "a Japanese b locale", 10))
        .await
        .expect("mixed keywords");
    assert!(
        recalled.iter().any(|item| item.slot_key == "edge.unicode"),
        "long keywords should match even when mixed with short ones"
    );

    // Edge 5: SQL injection attempt in query (should be safe via parameterized queries)
    let recalled = memory
        .recall_scoped(RecallQuery::new(
            "edge-entity",
            "'; DROP TABLE retrieval_units; --",
            10,
        ))
        .await
        .expect("sql injection attempt should not crash");
    assert!(
        recalled.is_empty(),
        "SQL injection should not return results or crash"
    );

    // Edge 6: Limit zero returns empty
    let recalled = memory
        .recall_scoped(RecallQuery::new("edge-entity", "Japanese", 0))
        .await
        .expect("zero limit");
    assert!(recalled.is_empty(), "zero limit should return nothing");
}

#[tokio::test]
async fn sqlite_raw_signal_promotes_to_candidate_after_corroboration() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    let entity_id = "promotion-entity";
    let slot_key = "profile.location.city";

    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                slot_key,
                MemoryEventType::FactAdded,
                "city=Kyoto",
                MemorySource::ExternalSecondary,
                PrivacyLevel::Private,
            )
            .with_signal_tier(SignalTier::Raw),
        )
        .await
        .expect("first raw event insert");

    let unit_id = format!("{entity_id}:{slot_key}");
    assert_eq!(promotion_status_for(&tmp, &unit_id).as_deref(), Some("raw"));

    memory
        .append_event(
            MemoryEventInput::new(
                entity_id,
                slot_key,
                MemoryEventType::FactAdded,
                "city=Kyoto",
                MemorySource::ExternalPrimary,
                PrivacyLevel::Private,
            )
            .with_signal_tier(SignalTier::Raw),
        )
        .await
        .expect("corroborating raw event insert");

    assert_eq!(
        promotion_status_for(&tmp, &unit_id).as_deref(),
        Some("candidate")
    );
}

#[tokio::test]
async fn sqlite_multi_phase_trend_boost_prefers_recent_raw_trend_candidate() {
    let tmp = TempDir::new().expect("tempdir");
    let memory = SqliteMemory::new(tmp.path()).expect("sqlite memory");
    let now = Utc::now();

    for source in [
        MemorySource::ExternalSecondary,
        MemorySource::ExternalPrimary,
    ] {
        memory
            .append_event(
                MemoryEventInput::new(
                    "pipeline-entity",
                    "trend.market.signal",
                    MemoryEventType::FactAdded,
                    "spike signal observed",
                    source,
                    PrivacyLevel::Private,
                )
                .with_signal_tier(SignalTier::Raw)
                .with_confidence(0.8)
                .with_importance(0.7)
                .with_occurred_at(now.to_rfc3339()),
            )
            .await
            .expect("insert trend candidate source");
    }

    for source in [
        MemorySource::ExternalSecondary,
        MemorySource::ExternalPrimary,
    ] {
        memory
            .append_event(
                MemoryEventInput::new(
                    "pipeline-entity",
                    "profile.signal.archive",
                    MemoryEventType::FactAdded,
                    "spike signal observed",
                    source,
                    PrivacyLevel::Private,
                )
                .with_signal_tier(SignalTier::Raw)
                .with_confidence(0.8)
                .with_importance(0.7)
                .with_occurred_at(now.to_rfc3339()),
            )
            .await
            .expect("insert non-trend candidate source");
    }

    let recalled = memory
        .recall_scoped(RecallQuery::new("pipeline-entity", "spike signal", 10))
        .await
        .expect("recall succeeds");

    assert!(
        recalled
            .iter()
            .any(|item| item.slot_key == "trend.market.signal"),
        "trend candidate should be recalled"
    );
    assert!(
        recalled
            .iter()
            .any(|item| item.slot_key == "profile.signal.archive"),
        "non-trend candidate should be recalled"
    );

    let trend_index = recalled
        .iter()
        .position(|item| item.slot_key == "trend.market.signal")
        .expect("trend candidate present");
    let non_trend_index = recalled
        .iter()
        .position(|item| item.slot_key == "profile.signal.archive")
        .expect("non-trend candidate present");
    assert!(trend_index < non_trend_index);
}

#[tokio::test]
async fn ingestion_pipeline_end_to_end() {
    let tmp = TempDir::new().expect("tempdir");
    let memory: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).expect("sqlite memory"));
    let pipeline = SqliteIngestionPipeline::new(Arc::clone(&memory));

    let content = "Discord channel reports user prefers Rust for backend services";
    let envelope = SignalEnvelope::new(
        SourceKind::Discord,
        "discord:channel-1",
        content,
        "ingestion-entity",
    );

    let result = pipeline.ingest(envelope).await.expect("ingest succeeds");
    assert!(result.accepted);
    assert_eq!(result.signal_tier, SignalTier::Raw);

    let corroboration_result = pipeline
        .ingest(
            SignalEnvelope::new(
                SourceKind::Discord,
                "discord:channel-1",
                content,
                "ingestion-entity",
            )
            .with_signal_tier(SignalTier::Belief),
        )
        .await
        .expect("corroborating ingest succeeds");
    assert!(corroboration_result.accepted);

    let recalled = memory
        .recall_scoped(RecallQuery::new("ingestion-entity", "prefers Rust", 10))
        .await
        .expect("recall succeeds");

    assert!(
        recalled.iter().any(|item| {
            item.slot_key == result.slot_key && item.value.contains("prefers Rust")
        }),
        "ingested content should be recalled"
    );
}
