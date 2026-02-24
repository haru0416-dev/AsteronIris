use super::{
    INGEST_API_MIN_INTERVAL_SECONDS, INGEST_RSS_MIN_INTERVAL_SECONDS, ROUTE_MARKER_INGEST_PIPELINE,
    ROUTE_MARKER_RSS_POLL, ROUTE_MARKER_TREND_AGGREGATION, ROUTE_MARKER_X_POLL,
    TREND_AGGREGATION_LIMIT, TREND_AGGREGATION_TOP_ITEMS, X_RECENT_SEARCH_ENDPOINT,
};
use crate::config::Config;
use crate::memory::factory::create_memory;
use crate::memory::ingestion::{IngestionPipeline, SignalEnvelope, SqliteIngestionPipeline};
use crate::memory::traits::Memory;
use crate::memory::types::{
    MemoryEventInput, MemoryEventType, MemoryLayer, MemoryProvenance, MemorySource, PrivacyLevel,
    RecallQuery, SourceKind,
};
use crate::security::SecurityPolicy;
use chrono::Utc;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

static INGEST_SOURCE_LAST_SEEN: LazyLock<Mutex<HashMap<String, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
struct XRecentSearchResponse {
    #[serde(default)]
    data: Vec<XRecentTweet>,
}

#[derive(Debug, Deserialize)]
pub(super) struct XRecentTweet {
    pub(super) id: String,
    pub(super) text: String,
    #[serde(default)]
    pub(super) lang: Option<String>,
    #[serde(default)]
    pub(super) author_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedIngestionJob {
    pub(super) source_kind: SourceKind,
    pub(super) entity_id: String,
    pub(super) source_ref: String,
    pub(super) content: String,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedTrendAggregationJob {
    pub(super) entity_id: String,
    pub(super) topic_key: String,
    pub(super) query: String,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedXPollJob {
    pub(super) entity_id: String,
    pub(super) query: String,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedRssPollJob {
    pub(super) entity_id: String,
    pub(super) url: String,
}

#[derive(Debug, Clone)]
pub(super) enum ParsedRoutedJob {
    Ingestion(ParsedIngestionJob),
    TrendAggregation(ParsedTrendAggregationJob),
    XPoll(ParsedXPollJob),
    RssPoll(ParsedRssPollJob),
}

#[derive(Debug, Clone)]
pub(super) struct RssPollItem {
    pub(super) source_ref: String,
    pub(super) content: String,
}

fn normalize_trend_topic_key(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_dot = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch.to_ascii_lowercase());
            last_dot = false;
        } else if !last_dot {
            out.push('.');
            last_dot = true;
        }
    }
    out.trim_matches('.').to_string()
}

const fn ingestion_min_interval_seconds(source_kind: SourceKind) -> i64 {
    match source_kind {
        SourceKind::News => INGEST_RSS_MIN_INTERVAL_SECONDS,
        SourceKind::Api
        | SourceKind::Conversation
        | SourceKind::Discord
        | SourceKind::Telegram
        | SourceKind::Slack
        | SourceKind::Document
        | SourceKind::Manual => INGEST_API_MIN_INTERVAL_SECONDS,
    }
}

fn check_and_record_ingestion_rate_limit(job: &ParsedIngestionJob) -> anyhow::Result<Option<i64>> {
    let key = format!("{}:{}", job.source_kind, job.source_ref);
    let now = Utc::now().timestamp();
    let interval = ingestion_min_interval_seconds(job.source_kind);

    let mut tracker = INGEST_SOURCE_LAST_SEEN
        .lock()
        .map_err(|e| anyhow::anyhow!("ingestion rate-limit tracker lock poisoned: {e}"))?;
    let Some(previous) = tracker.get(&key).copied() else {
        tracker.insert(key, now);
        return Ok(None);
    };

    let elapsed = now.saturating_sub(previous);
    if elapsed >= interval {
        tracker.insert(key, now);
        return Ok(None);
    }

    Ok(Some(interval - elapsed))
}

fn consume_security_or_output(
    security: &SecurityPolicy,
    route_marker: &str,
) -> Option<(bool, String)> {
    security
        .consume_action_and_cost(0)
        .err()
        .map(|policy_error| {
            (
                false,
                format!("{route_marker}\nblocked by security policy: {policy_error}"),
            )
        })
}

async fn create_memory_or_output(
    config: &Config,
    route_marker: &str,
) -> Result<Box<dyn Memory>, (bool, String)> {
    create_memory(&config.memory, &config.workspace_dir, None)
        .await
        .map_err(|error| {
            (
                false,
                format!("{route_marker}\ncreate_memory failed: {error}"),
            )
        })
}

async fn create_ingestion_pipeline_or_output(
    config: &Config,
    route_marker: &str,
) -> Result<SqliteIngestionPipeline, (bool, String)> {
    let memory = create_memory_or_output(config, route_marker).await?;
    Ok(SqliteIngestionPipeline::new(Arc::from(memory)))
}

pub(super) fn parse_routed_job_command(command: &str) -> Option<ParsedRoutedJob> {
    let trimmed = command.trim();
    let (source_kind, rest, source_ref_prefix) =
        if let Some(rest) = trimmed.strip_prefix("ingest:api ") {
            (Some(SourceKind::Api), rest, "")
        } else if let Some(rest) = trimmed.strip_prefix("ingest:x ") {
            (Some(SourceKind::Api), rest, "x:")
        } else if let Some(rest) = trimmed.strip_prefix("ingest:x-poll ") {
            (None, rest, "x-poll")
        } else if let Some(rest) = trimmed.strip_prefix("ingest:rss-poll ") {
            (None, rest, "rss-poll")
        } else if let Some(rest) = trimmed.strip_prefix("ingest:rss ") {
            (Some(SourceKind::News), rest, "")
        } else if let Some(rest) = trimmed.strip_prefix("ingest:trend ") {
            (None, rest, "")
        } else {
            return None;
        };

    if source_ref_prefix == "x-poll" {
        let mut parts = rest.splitn(2, ' ');
        let entity_id = parts.next()?.trim();
        let query = parts.next()?.trim();
        if entity_id.is_empty() || query.is_empty() {
            return None;
        }

        return Some(ParsedRoutedJob::XPoll(ParsedXPollJob {
            entity_id: entity_id.to_string(),
            query: query.to_string(),
        }));
    }

    if source_ref_prefix == "rss-poll" {
        let mut parts = rest.splitn(2, ' ');
        let entity_id = parts.next()?.trim();
        let url = parts.next()?.trim();
        if entity_id.is_empty() || url.is_empty() {
            return None;
        }

        return Some(ParsedRoutedJob::RssPoll(ParsedRssPollJob {
            entity_id: entity_id.to_string(),
            url: url.to_string(),
        }));
    }

    if let Some(source_kind) = source_kind {
        let mut parts = rest.splitn(3, ' ');
        let entity_id = parts.next()?.trim();
        let source_ref = parts.next()?.trim();
        let content = parts.next()?.trim();
        if entity_id.is_empty() || source_ref.is_empty() || content.is_empty() {
            return None;
        }

        return Some(ParsedRoutedJob::Ingestion(ParsedIngestionJob {
            source_kind,
            entity_id: entity_id.to_string(),
            source_ref: format!("{source_ref_prefix}{source_ref}"),
            content: content.to_string(),
        }));
    }

    let mut parts = rest.splitn(3, ' ');
    let entity_id = parts.next()?.trim();
    let topic_key = normalize_trend_topic_key(parts.next()?.trim());
    let query = parts.next()?.trim();
    if entity_id.is_empty() || topic_key.is_empty() || query.is_empty() {
        return None;
    }

    Some(ParsedRoutedJob::TrendAggregation(
        ParsedTrendAggregationJob {
            entity_id: entity_id.to_string(),
            topic_key,
            query: query.to_string(),
        },
    ))
}

pub(super) fn resolve_x_bearer_token(raw: Option<String>) -> Result<String, String> {
    match raw {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(format!("{ROUTE_MARKER_X_POLL}\nmissing X_BEARER_TOKEN")),
    }
}

pub(super) fn resolve_x_recent_search_endpoint(raw: Option<String>) -> String {
    match raw {
        Some(value) if !value.trim().is_empty() => value,
        _ => X_RECENT_SEARCH_ENDPOINT.to_string(),
    }
}

pub(super) fn build_x_poll_envelopes(
    entity_id: &str,
    query: &str,
    tweets: Vec<XRecentTweet>,
) -> Vec<SignalEnvelope> {
    tweets
        .into_iter()
        .map(|tweet| {
            let mut envelope = SignalEnvelope::new(
                SourceKind::Api,
                format!("x:{}", tweet.id),
                tweet.text,
                entity_id.to_string(),
            )
            .with_privacy_level(PrivacyLevel::Private)
            .with_metadata("x_query", query.to_string());

            if let Some(author_id) = tweet.author_id {
                envelope = envelope.with_metadata("x_author_id", author_id);
            }
            if let Some(lang) = tweet.lang {
                envelope = envelope.with_language(lang);
            }
            envelope
        })
        .collect::<Vec<_>>()
}

fn decode_cdata(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|value| value.strip_suffix("]]>"))
    {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_xml_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let rest = &block[start..];
    let end = rest.find(&close)?;
    Some(decode_cdata(&rest[..end]))
}

pub(super) fn parse_rss_items_from_xml(xml: &str, limit: usize) -> Vec<RssPollItem> {
    if limit == 0 {
        return Vec::new();
    }

    let mut items = Vec::new();
    for chunk in xml.split("<item").skip(1) {
        if items.len() >= limit {
            break;
        }
        let Some((_, body)) = chunk.split_once('>') else {
            continue;
        };
        let Some((item_body, _)) = body.split_once("</item>") else {
            continue;
        };

        let title = extract_xml_tag(item_body, "title").unwrap_or_default();
        let description = extract_xml_tag(item_body, "description").unwrap_or_default();
        let guid = extract_xml_tag(item_body, "guid");
        let link = extract_xml_tag(item_body, "link");

        let id = guid
            .or(link)
            .unwrap_or_else(|| format!("rss-item-{}", items.len() + 1));
        let content = if !title.is_empty() && !description.is_empty() {
            format!("{title} - {description}")
        } else if !title.is_empty() {
            title
        } else {
            description
        };

        if content.trim().is_empty() {
            continue;
        }

        items.push(RssPollItem {
            source_ref: format!("rss:{id}"),
            content,
        });
    }

    items
}

pub(super) fn build_rss_poll_envelopes(
    entity_id: &str,
    items: Vec<RssPollItem>,
) -> Vec<SignalEnvelope> {
    items
        .into_iter()
        .map(|item| {
            SignalEnvelope::new(
                SourceKind::News,
                item.source_ref,
                item.content,
                entity_id.to_string(),
            )
            .with_privacy_level(PrivacyLevel::Private)
        })
        .collect::<Vec<_>>()
}

pub(super) async fn run_ingestion_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedIngestionJob,
) -> (bool, String) {
    match check_and_record_ingestion_rate_limit(&job) {
        Ok(Some(wait_seconds)) => {
            return (
                false,
                format!(
                    "{ROUTE_MARKER_INGEST_PIPELINE}\naccepted=false\nreason=rate_limited\nwait_seconds={wait_seconds}"
                ),
            );
        }
        Ok(None) => {}
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_INGEST_PIPELINE}\nrate limiter failed: {error}"),
            );
        }
    }

    if let Some(output) = consume_security_or_output(security, ROUTE_MARKER_INGEST_PIPELINE) {
        return output;
    }

    let pipeline =
        match create_ingestion_pipeline_or_output(config, ROUTE_MARKER_INGEST_PIPELINE).await {
            Ok(pipeline) => pipeline,
            Err(output) => return output,
        };
    let envelope = SignalEnvelope::new(job.source_kind, job.source_ref, job.content, job.entity_id)
        .with_privacy_level(PrivacyLevel::Private);

    match pipeline.ingest(envelope).await {
        Ok(result) => (
            result.accepted,
            format!(
                "{ROUTE_MARKER_INGEST_PIPELINE}\naccepted={}\nslot_key={}\nreason={}",
                result.accepted,
                result.slot_key,
                result.reason.unwrap_or_else(|| "none".to_string())
            ),
        ),
        Err(error) => (
            false,
            format!("{ROUTE_MARKER_INGEST_PIPELINE}\ningestion failed: {error}"),
        ),
    }
}

pub(super) async fn run_trend_aggregation_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedTrendAggregationJob,
) -> (bool, String) {
    if let Some(output) = consume_security_or_output(security, ROUTE_MARKER_TREND_AGGREGATION) {
        return output;
    }

    let memory = match create_memory_or_output(config, ROUTE_MARKER_TREND_AGGREGATION).await {
        Ok(memory) => memory,
        Err(output) => return output,
    };

    let recalled = match memory
        .recall_scoped(RecallQuery::new(
            &job.entity_id,
            &job.query,
            TREND_AGGREGATION_LIMIT,
        ))
        .await
    {
        Ok(items) => items,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_TREND_AGGREGATION}\nrecall_scoped failed: {error}"),
            );
        }
    };

    let mut candidates = recalled
        .into_iter()
        .filter(|item| item.slot_key.starts_with("external."))
        .collect::<Vec<_>>();
    candidates.truncate(TREND_AGGREGATION_TOP_ITEMS);

    if candidates.is_empty() {
        return (
            true,
            format!(
                "{ROUTE_MARKER_TREND_AGGREGATION}\naccepted=false\nreason=no_external_candidates"
            ),
        );
    }

    let slot_key = format!("trend.snapshot.{}", job.topic_key);
    let summary = candidates
        .iter()
        .map(|item| {
            format!(
                "{}({:.2}):{}",
                item.slot_key,
                item.score,
                item.value.replace('\n', " ")
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    let payload = format!(
        "trend topic={} query='{}' candidates={} top={}",
        job.topic_key,
        job.query,
        candidates.len(),
        summary
    );

    let input = MemoryEventInput::new(
        &job.entity_id,
        &slot_key,
        MemoryEventType::SummaryCompacted,
        payload,
        MemorySource::System,
        PrivacyLevel::Private,
    )
    .with_layer(MemoryLayer::Working)
    .with_importance(0.6)
    .with_provenance(MemoryProvenance::source_reference(
        MemorySource::System,
        format!("ingestion:trend:{}", job.topic_key),
    ));

    match memory.append_event(input).await {
        Ok(_) => (
            true,
            format!(
                "{ROUTE_MARKER_TREND_AGGREGATION}\naccepted=true\nslot_key={slot_key}\nsource_count={}\nquery={}",
                candidates.len(),
                job.query
            ),
        ),
        Err(error) => (
            false,
            format!("{ROUTE_MARKER_TREND_AGGREGATION}\nappend_event failed: {error}"),
        ),
    }
}

pub(super) async fn run_rss_poll_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedRssPollJob,
) -> (bool, String) {
    if let Some(output) = consume_security_or_output(security, ROUTE_MARKER_RSS_POLL) {
        return output;
    }

    let response = match reqwest::get(&job.url).await {
        Ok(resp) => resp,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_RSS_POLL}\nrequest failed: {error}"),
            );
        }
    };
    if !response.status().is_success() {
        return (
            false,
            format!(
                "{ROUTE_MARKER_RSS_POLL}\nrss fetch non-success status={}",
                response.status()
            ),
        );
    }

    let xml = match response.text().await {
        Ok(body) => body,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_RSS_POLL}\nresponse decode failed: {error}"),
            );
        }
    };

    let items = parse_rss_items_from_xml(&xml, 10);
    if items.is_empty() {
        return (
            true,
            format!("{ROUTE_MARKER_RSS_POLL}\naccepted=false\nreason=no_items"),
        );
    }

    let pipeline = match create_ingestion_pipeline_or_output(config, ROUTE_MARKER_RSS_POLL).await {
        Ok(pipeline) => pipeline,
        Err(output) => return output,
    };

    let envelopes = build_rss_poll_envelopes(&job.entity_id, items);
    match pipeline.ingest_batch(envelopes).await {
        Ok(results) => {
            let accepted_count = results.iter().filter(|item| item.accepted).count();
            (
                true,
                format!(
                    "{ROUTE_MARKER_RSS_POLL}\naccepted=true\naccepted_count={accepted_count}\ntotal={}\nurl={}",
                    results.len(),
                    job.url
                ),
            )
        }
        Err(error) => (
            false,
            format!("{ROUTE_MARKER_RSS_POLL}\ningestion batch failed: {error}"),
        ),
    }
}

#[allow(clippy::too_many_lines)]
pub(super) async fn run_x_poll_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedXPollJob,
) -> (bool, String) {
    if let Some(output) = consume_security_or_output(security, ROUTE_MARKER_X_POLL) {
        return output;
    }

    let token = match resolve_x_bearer_token(std::env::var("X_BEARER_TOKEN").ok()) {
        Ok(token) => token,
        Err(output) => return (false, output),
    };

    let endpoint = resolve_x_recent_search_endpoint(
        std::env::var("ASTERONIRIS_X_RECENT_SEARCH_ENDPOINT").ok(),
    );

    let client = reqwest::Client::new();
    let response = client
        .get(endpoint)
        .bearer_auth(token)
        .query(&[
            ("query", job.query.as_str()),
            ("max_results", "10"),
            ("tweet.fields", "created_at,lang,author_id"),
        ])
        .send()
        .await;
    let response = match response {
        Ok(resp) => resp,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_X_POLL}\nrequest failed: {error}"),
            );
        }
    };

    if !response.status().is_success() {
        return (
            false,
            format!(
                "{ROUTE_MARKER_X_POLL}\nx api non-success status={}",
                response.status()
            ),
        );
    }

    let parsed: XRecentSearchResponse = match response.json().await {
        Ok(parsed) => parsed,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_X_POLL}\nresponse decode failed: {error}"),
            );
        }
    };

    if parsed.data.is_empty() {
        return (
            true,
            format!("{ROUTE_MARKER_X_POLL}\naccepted=false\nreason=no_tweets"),
        );
    }

    let pipeline = match create_ingestion_pipeline_or_output(config, ROUTE_MARKER_X_POLL).await {
        Ok(pipeline) => pipeline,
        Err(output) => return output,
    };

    let envelopes = build_x_poll_envelopes(&job.entity_id, &job.query, parsed.data);

    match pipeline.ingest_batch(envelopes).await {
        Ok(results) => {
            let accepted_count = results.iter().filter(|item| item.accepted).count();
            (
                true,
                format!(
                    "{ROUTE_MARKER_X_POLL}\naccepted=true\naccepted_count={accepted_count}\ntotal={}\nquery={}",
                    results.len(),
                    job.query
                ),
            )
        }
        Err(error) => (
            false,
            format!("{ROUTE_MARKER_X_POLL}\ningestion batch failed: {error}"),
        ),
    }
}
