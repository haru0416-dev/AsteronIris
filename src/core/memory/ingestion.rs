use crate::core::memory::memory_types::{
    MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel, RecallQuery,
    SignalTier, SourceKind,
};
use crate::core::memory::traits::{Memory, MemoryLayer};
use crate::runtime::observability::traits::{AutonomyLifecycleSignal, ObserverMetric};
use crate::runtime::observability::{NoopObserver, Observer};
use crate::security::writeback_guard::enforce_ingestion_write_policy;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEnvelope {
    pub source_kind: SourceKind,
    pub source_ref: String,
    pub content: String,
    pub entity_id: String,
    pub signal_tier: SignalTier,
    pub privacy_level: PrivacyLevel,
    pub language: Option<String>,
    pub metadata: HashMap<String, String>,
    pub ingested_at: String,
}

impl SignalEnvelope {
    pub fn new(
        source_kind: SourceKind,
        source_ref: impl Into<String>,
        content: impl Into<String>,
        entity_id: impl Into<String>,
    ) -> Self {
        Self {
            source_kind,
            source_ref: source_ref.into(),
            content: content.into(),
            entity_id: entity_id.into(),
            signal_tier: SignalTier::Raw,
            privacy_level: PrivacyLevel::Public,
            language: None,
            metadata: HashMap::new(),
            ingested_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_signal_tier(mut self, tier: SignalTier) -> Self {
        self.signal_tier = tier;
        self
    }

    pub fn with_privacy_level(mut self, level: PrivacyLevel) -> Self {
        self.privacy_level = level;
        self
    }

    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn source_kind_str(&self) -> String {
        self.source_kind.to_string()
    }

    pub fn normalize(mut self) -> anyhow::Result<Self> {
        self.source_ref = normalize_source_ref(&self.source_ref)?;
        self.content = normalize_content(&self.content)?;
        self.entity_id = normalize_entity_id(&self.entity_id)?;
        self.language = self
            .language
            .as_deref()
            .map(normalize_language)
            .transpose()?;
        self.ingested_at = normalize_ingested_at(&self.ingested_at);
        self.apply_rule_based_classification();
        Ok(self)
    }

    fn apply_rule_based_classification(&mut self) {
        let content_lower = self.content.to_ascii_lowercase();

        let mut risk_flags = Vec::new();
        if contains_any(
            &content_lower,
            &["rumor", "unverified", "allegedly", "未確認", "噂"],
        ) {
            risk_flags.push("rumor");
            risk_flags.push("unverified");
        }
        if contains_any(
            &content_lower,
            &[
                "password",
                "api key",
                "token",
                "secret",
                "個人情報",
                "住所",
                "電話番号",
            ],
        ) {
            risk_flags.push("sensitive");
        }
        if contains_any(
            &content_lower,
            &[
                "policy",
                "ban",
                "compliance",
                "regulation",
                "利用規約",
                "コンプライアンス",
            ],
        ) {
            risk_flags.push("policy_risky");
        }
        if !risk_flags.is_empty() {
            risk_flags.sort_unstable();
            risk_flags.dedup();
            self.metadata
                .insert("risk_flags".to_string(), risk_flags.join("|"));
        }

        let topic = infer_topic(&content_lower, self.source_kind);
        self.metadata
            .entry("topic".to_string())
            .or_insert(topic.to_string());

        let entity = infer_entity_hint(&self.entity_id);
        self.metadata
            .entry("entity_hint".to_string())
            .or_insert(entity);
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn infer_topic(content_lower: &str, source_kind: SourceKind) -> &'static str {
    if contains_any(
        content_lower,
        &[
            "security",
            "vulnerability",
            "exploit",
            "脆弱性",
            "セキュリティ",
        ],
    ) {
        return "security";
    }
    if contains_any(
        content_lower,
        &["release", "version", "deploy", "リリース", "デプロイ"],
    ) {
        return "release";
    }
    if contains_any(content_lower, &["price", "market", "stocks", "株", "相場"]) {
        return "market";
    }

    match source_kind {
        SourceKind::News => "news",
        SourceKind::Document => "document",
        SourceKind::Conversation => "conversation",
        SourceKind::Discord | SourceKind::Telegram | SourceKind::Slack => "community",
        SourceKind::Api => "api",
        SourceKind::Manual => "manual",
    }
}

fn infer_entity_hint(entity_id: &str) -> String {
    if let Some((prefix, rest)) = entity_id.split_once(':')
        && !rest.is_empty()
    {
        return format!("{prefix}:{rest}");
    }
    entity_id.to_string()
}

fn normalize_source_ref(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_identifier(raw, true);
    if normalized.is_empty() {
        anyhow::bail!("signal_envelope.source_ref must not be empty");
    }
    if normalized.len() > 256 {
        anyhow::bail!("signal_envelope.source_ref must be <= 256 chars");
    }
    Ok(normalized)
}

fn normalize_content(raw: &str) -> anyhow::Result<String> {
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        anyhow::bail!("signal_envelope.content must not be empty");
    }
    Ok(normalized)
}

fn normalize_entity_id(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_identifier(raw, false);
    if normalized.is_empty() {
        anyhow::bail!("signal_envelope.entity_id must not be empty");
    }
    if normalized.len() > 128 {
        anyhow::bail!("signal_envelope.entity_id must be <= 128 chars");
    }
    Ok(normalized)
}

fn normalize_language(raw: &str) -> anyhow::Result<String> {
    let candidate = raw.trim().to_ascii_lowercase();
    let normalized = candidate
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '-')
        .collect::<String>();
    if normalized.is_empty() {
        anyhow::bail!("signal_envelope.language must contain at least one valid character");
    }
    if normalized.len() > 16 {
        anyhow::bail!("signal_envelope.language must be <= 16 chars");
    }
    Ok(normalized)
}

fn normalize_ingested_at(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map_or_else(|_| chrono::Utc::now().to_rfc3339(), |dt| dt.to_rfc3339())
}

fn normalize_identifier(raw: &str, allow_slash: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_underscore = false;
    for ch in raw.trim().chars() {
        let allowed = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '_' | '-' | ':')
            || (allow_slash && ch == '/');
        if allowed {
            out.push(ch);
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionResult {
    pub accepted: bool,
    pub slot_key: String,
    pub signal_tier: SignalTier,
    pub reason: Option<String>,
}

#[async_trait]
pub trait IngestionPipeline: Send + Sync {
    async fn ingest(&self, envelope: SignalEnvelope) -> anyhow::Result<IngestionResult>;

    async fn ingest_batch(
        &self,
        envelopes: Vec<SignalEnvelope>,
    ) -> anyhow::Result<Vec<IngestionResult>> {
        let mut results = Vec::with_capacity(envelopes.len());
        for envelope in envelopes {
            results.push(self.ingest(envelope).await?);
        }
        Ok(results)
    }
}

#[derive(Clone)]
pub struct SqliteIngestionPipeline {
    memory: Arc<dyn Memory>,
    semantic_dedup_cache: Arc<Mutex<HashSet<String>>>,
    observer: Arc<dyn Observer>,
}

impl SqliteIngestionPipeline {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self::new_with_observer(memory, Arc::new(NoopObserver))
    }

    pub fn new_with_observer(memory: Arc<dyn Memory>, observer: Arc<dyn Observer>) -> Self {
        Self {
            memory,
            semantic_dedup_cache: Arc::new(Mutex::new(HashSet::new())),
            observer,
        }
    }

    async fn is_source_ref_exact_duplicate(
        &self,
        envelope: &SignalEnvelope,
        slot_key: &str,
    ) -> anyhow::Result<bool> {
        let existing = self
            .memory
            .resolve_slot(&envelope.entity_id, slot_key)
            .await?;
        Ok(existing.is_some_and(|slot| slot.value == envelope.content))
    }

    async fn is_semantic_duplicate(
        &self,
        envelope: &SignalEnvelope,
        slot_key: &str,
    ) -> anyhow::Result<bool> {
        let source_kind_prefix = format!("external.{}.", envelope.source_kind_str());
        let semantic_candidates = self
            .memory
            .recall_scoped(RecallQuery::new(&envelope.entity_id, &envelope.content, 5))
            .await?;

        Ok(semantic_candidates.iter().any(|item| {
            item.slot_key != slot_key
                && item.slot_key.starts_with(&source_kind_prefix)
                && (item.value == envelope.content || item.score >= 0.95)
        }))
    }

    fn dedup_cache_contains(&self, semantic_key: &str) -> anyhow::Result<bool> {
        let cache = self
            .semantic_dedup_cache
            .lock()
            .map_err(|e| anyhow::anyhow!("semantic dedup cache lock poisoned: {e}"))?;
        Ok(cache.contains(semantic_key))
    }

    fn dedup_cache_insert(&self, semantic_key: String) -> anyhow::Result<()> {
        let mut cache = self
            .semantic_dedup_cache
            .lock()
            .map_err(|e| anyhow::anyhow!("semantic dedup cache lock poisoned: {e}"))?;
        cache.insert(semantic_key);
        Ok(())
    }
}

fn semantic_dedup_key(entity_id: &str, source_kind: SourceKind, content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(entity_id.as_bytes());
    hasher.update(b"::");
    hasher.update(source_kind.to_string().as_bytes());
    hasher.update(b"::");
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    format!("{digest:x}")
}

#[async_trait]
impl IngestionPipeline for SqliteIngestionPipeline {
    async fn ingest(&self, envelope: SignalEnvelope) -> anyhow::Result<IngestionResult> {
        let envelope = envelope.normalize()?;
        let source_class = match envelope.source_kind {
            SourceKind::Conversation | SourceKind::Manual => MemorySource::ExplicitUser,
            SourceKind::Discord | SourceKind::Telegram | SourceKind::Slack => {
                MemorySource::ExternalPrimary
            }
            SourceKind::Api | SourceKind::News | SourceKind::Document => {
                MemorySource::ExternalSecondary
            }
        };

        let slot_key = format!(
            "external.{}.{}",
            envelope.source_kind_str(),
            envelope.source_ref
        );
        let source_kind_label = envelope.source_kind_str();
        let semantic_key =
            semantic_dedup_key(&envelope.entity_id, envelope.source_kind, &envelope.content);

        if envelope.signal_tier == SignalTier::Raw
            && self
                .is_source_ref_exact_duplicate(&envelope, &slot_key)
                .await?
        {
            self.observer
                .record_autonomy_lifecycle(AutonomyLifecycleSignal::Deduplicated);
            self.observer
                .record_metric(&ObserverMetric::SignalDedupDropTotal {
                    source_kind: source_kind_label.clone(),
                });
            return Ok(IngestionResult {
                accepted: false,
                slot_key,
                signal_tier: envelope.signal_tier,
                reason: Some("dedup:source_ref_exact".to_string()),
            });
        }

        if envelope.signal_tier == SignalTier::Raw
            && self.is_semantic_duplicate(&envelope, &slot_key).await?
        {
            self.dedup_cache_insert(semantic_key.clone())?;
            self.observer
                .record_autonomy_lifecycle(AutonomyLifecycleSignal::Deduplicated);
            self.observer
                .record_metric(&ObserverMetric::SignalDedupDropTotal {
                    source_kind: source_kind_label.clone(),
                });
            return Ok(IngestionResult {
                accepted: false,
                slot_key,
                signal_tier: envelope.signal_tier,
                reason: Some("dedup:semantic_similar".to_string()),
            });
        }

        if envelope.signal_tier == SignalTier::Raw && self.dedup_cache_contains(&semantic_key)? {
            self.observer
                .record_autonomy_lifecycle(AutonomyLifecycleSignal::Deduplicated);
            self.observer
                .record_metric(&ObserverMetric::SignalDedupDropTotal {
                    source_kind: source_kind_label.clone(),
                });
            return Ok(IngestionResult {
                accepted: false,
                slot_key,
                signal_tier: envelope.signal_tier,
                reason: Some("dedup:semantic_similar".to_string()),
            });
        }

        let source_ref = envelope.source_ref;

        let input = MemoryEventInput::new(
            envelope.entity_id,
            &slot_key,
            MemoryEventType::FactAdded,
            envelope.content,
            source_class,
            envelope.privacy_level,
        )
        .with_signal_tier(envelope.signal_tier)
        .with_source_kind(envelope.source_kind)
        .with_source_ref(&source_ref)
        .with_layer(MemoryLayer::Working)
        .with_importance(0.4)
        .with_provenance(MemoryProvenance::source_reference(
            source_class,
            format!("ingestion:{source_ref}"),
        ));

        enforce_ingestion_write_policy(&input)?;

        self.memory.append_event(input).await?;
        self.dedup_cache_insert(semantic_key)?;
        self.observer
            .record_autonomy_lifecycle(AutonomyLifecycleSignal::Ingested);
        self.observer
            .record_metric(&ObserverMetric::SignalIngestTotal {
                source_kind: source_kind_label,
            });

        Ok(IngestionResult {
            accepted: true,
            slot_key,
            signal_tier: envelope.signal_tier,
            reason: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::memory::SqliteMemory;
    use crate::runtime::observability::PrometheusObserver;
    use tempfile::TempDir;

    #[tokio::test]
    async fn ingestion_pipeline_writes_valid_envelope() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
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
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let pipeline = SqliteIngestionPipeline::new(mem);

        let envelope = SignalEnvelope::new(SourceKind::Api, "   ", "payload", "person:api.test");
        let err = pipeline.ingest(envelope).await.unwrap_err().to_string();
        assert!(err.contains("source_ref"));
    }

    #[tokio::test]
    async fn ingestion_pipeline_rejects_source_ref_that_sanitizes_to_empty() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let pipeline = SqliteIngestionPipeline::new(mem);

        let envelope = SignalEnvelope::new(SourceKind::Api, "???", "payload", "person:api.test");
        let err = pipeline.ingest(envelope).await.unwrap_err().to_string();
        assert!(err.contains("signal_envelope.source_ref must not be empty"));
    }

    #[tokio::test]
    async fn ingestion_pipeline_drops_exact_source_ref_duplicates() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
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
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
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
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
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
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
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

    #[tokio::test]
    async fn ingestion_pipeline_records_ingested_and_deduplicated_signals() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let observer = Arc::new(PrometheusObserver::new());
        let pipeline = SqliteIngestionPipeline::new_with_observer(mem, observer.clone());

        let envelope = SignalEnvelope::new(
            SourceKind::Api,
            "api:item-observe",
            "observe payload",
            "person:obs.test",
        );

        let first = pipeline.ingest(envelope.clone()).await.unwrap();
        assert!(first.accepted);
        let second = pipeline.ingest(envelope).await.unwrap();
        assert!(!second.accepted);

        let counts = observer.snapshot_autonomy_counts();
        assert!(counts.ingested >= 1);
        assert!(counts.deduplicated >= 1);

        let signal_counts = observer.snapshot_signal_counts();
        assert!(
            signal_counts
                .ingested_by_source
                .get("api")
                .copied()
                .unwrap_or(0)
                >= 1
        );
        assert!(
            signal_counts
                .dedup_drop_by_source
                .get("api")
                .copied()
                .unwrap_or(0)
                >= 1
        );
    }

    #[tokio::test]
    async fn ingestion_pipeline_records_source_metrics_per_kind() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let observer = Arc::new(PrometheusObserver::new());
        let pipeline = SqliteIngestionPipeline::new_with_observer(mem, observer.clone());

        let api = SignalEnvelope::new(
            SourceKind::Api,
            "api:source-kind-1",
            "api payload",
            "person:kind.test",
        );
        let news = SignalEnvelope::new(
            SourceKind::News,
            "news:source-kind-1",
            "news payload",
            "person:kind.test",
        );

        let api_result = pipeline.ingest(api).await.unwrap();
        assert!(api_result.accepted);
        let news_result = pipeline.ingest(news).await.unwrap();
        assert!(news_result.accepted);

        let signal_counts = observer.snapshot_signal_counts();
        assert_eq!(signal_counts.ingested_by_source.get("api"), Some(&1));
        assert_eq!(signal_counts.ingested_by_source.get("news"), Some(&1));
    }

    #[tokio::test]
    async fn ingestion_pipeline_records_source_metrics_for_manual_and_discord_kinds() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let observer = Arc::new(PrometheusObserver::new());
        let pipeline = SqliteIngestionPipeline::new_with_observer(mem, observer.clone());

        let manual = SignalEnvelope::new(
            SourceKind::Manual,
            "manual:source-kind-1",
            "manual payload",
            "person:kind.manual.discord",
        );
        let discord = SignalEnvelope::new(
            SourceKind::Discord,
            "discord:source-kind-1",
            "discord payload",
            "person:kind.manual.discord",
        );

        let manual_result = pipeline.ingest(manual).await.unwrap();
        assert!(manual_result.accepted);
        let discord_result = pipeline.ingest(discord).await.unwrap();
        assert!(discord_result.accepted);

        let signal_counts = observer.snapshot_signal_counts();
        assert_eq!(signal_counts.ingested_by_source.get("manual"), Some(&1));
        assert_eq!(signal_counts.ingested_by_source.get("discord"), Some(&1));
    }

    #[tokio::test]
    async fn ingestion_pipeline_records_dedup_drop_metrics_per_kind() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let observer = Arc::new(PrometheusObserver::new());
        let pipeline = SqliteIngestionPipeline::new_with_observer(mem, observer.clone());

        let api = SignalEnvelope::new(
            SourceKind::Api,
            "api:source-kind-dedup-1",
            "api dedup payload",
            "person:kind.test",
        );
        let news = SignalEnvelope::new(
            SourceKind::News,
            "news:source-kind-dedup-1",
            "news dedup payload",
            "person:kind.test",
        );

        assert!(pipeline.ingest(api.clone()).await.unwrap().accepted);
        assert!(pipeline.ingest(news.clone()).await.unwrap().accepted);
        assert!(!pipeline.ingest(api).await.unwrap().accepted);
        assert!(!pipeline.ingest(news).await.unwrap().accepted);

        let signal_counts = observer.snapshot_signal_counts();
        assert_eq!(signal_counts.dedup_drop_by_source.get("api"), Some(&1));
        assert_eq!(signal_counts.dedup_drop_by_source.get("news"), Some(&1));
    }

    #[tokio::test]
    async fn ingestion_pipeline_records_dedup_drop_metrics_for_manual_kind() {
        let tmp = TempDir::new().unwrap();
        let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(tmp.path()).unwrap());
        let observer = Arc::new(PrometheusObserver::new());
        let pipeline = SqliteIngestionPipeline::new_with_observer(mem, observer.clone());

        let manual = SignalEnvelope::new(
            SourceKind::Manual,
            "manual:source-kind-dedup-1",
            "manual dedup payload",
            "person:kind.manual.test",
        );

        assert!(pipeline.ingest(manual.clone()).await.unwrap().accepted);
        assert!(!pipeline.ingest(manual).await.unwrap().accepted);

        let signal_counts = observer.snapshot_signal_counts();
        assert_eq!(signal_counts.dedup_drop_by_source.get("manual"), Some(&1));
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
        let key_other_source =
            semantic_dedup_key("person:key.test", SourceKind::News, "same content");
        let key_other_entity =
            semantic_dedup_key("person:key.other", SourceKind::Api, "same content");
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
}
