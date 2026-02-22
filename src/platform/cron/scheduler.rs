use crate::config::Config;
use crate::core::memory::traits::MemoryLayer;
use crate::platform::cron::{CronJob, due_jobs, reschedule_after_run};
use crate::security::SecurityPolicy;
use crate::{
    core::memory::{
        IngestionPipeline, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource,
        PrivacyLevel, RecallQuery, SignalEnvelope, SourceKind, SqliteIngestionPipeline,
        create_memory,
    },
    core::planner::{PlanExecutor, PlanParser, ToolStepRunner},
    core::tools::{ToolRegistry, default_middleware_chain, default_tools},
    runtime::observability::create_observer,
};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::process::Command;
use tokio::time::{self, Duration};
use uuid::Uuid;

const MIN_POLL_SECONDS: u64 = 5;
const ROUTE_MARKER_USER_SHELL: &str = "route=user-direct-shell";
const ROUTE_MARKER_AGENT_BLOCKED: &str = "route=agent-no-direct-shell";
const ROUTE_MARKER_AGENT_PLANNER: &str = "route=agent-planner";
const ROUTE_MARKER_INGEST_PIPELINE: &str = "route=user-ingestion-pipeline";
const ROUTE_MARKER_TREND_AGGREGATION: &str = "route=user-trend-aggregation";
const ROUTE_MARKER_X_POLL: &str = "route=user-x-poll";
const ROUTE_MARKER_RSS_POLL: &str = "route=user-rss-poll";
const TREND_AGGREGATION_LIMIT: usize = 20;
const TREND_AGGREGATION_TOP_ITEMS: usize = 5;
const INGEST_API_MIN_INTERVAL_SECONDS: i64 = 10;
const INGEST_RSS_MIN_INTERVAL_SECONDS: i64 = 30;

static INGEST_SOURCE_LAST_SEEN: LazyLock<Mutex<HashMap<String, i64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const X_RECENT_SEARCH_ENDPOINT: &str = "https://api.twitter.com/2/tweets/search/recent";

#[derive(Debug, Deserialize)]
struct XRecentSearchResponse {
    #[serde(default)]
    data: Vec<XRecentTweet>,
}

#[derive(Debug, Deserialize)]
struct XRecentTweet {
    id: String,
    text: String,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    author_id: Option<String>,
}

pub async fn run(config: Arc<Config>) -> Result<()> {
    let poll_secs = config.reliability.scheduler_poll_secs.max(MIN_POLL_SECONDS);
    let mut interval = time::interval(Duration::from_secs(poll_secs));
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    initialize_scheduler_state(&config);

    crate::runtime::diagnostics::health::mark_component_ok("scheduler");

    loop {
        interval.tick().await;
        crate::runtime::diagnostics::health::mark_component_ok("scheduler");

        let jobs = match due_jobs(&config, Utc::now()) {
            Ok(jobs) => jobs,
            Err(e) => {
                crate::runtime::diagnostics::health::mark_component_error(
                    "scheduler",
                    e.to_string(),
                );
                tracing::warn!("Scheduler query failed: {e}");
                continue;
            }
        };

        for job in jobs {
            crate::runtime::diagnostics::health::mark_component_ok("scheduler");
            let (success, output) = execute_job_with_retry(&config, &security, &job).await;

            if !success {
                crate::runtime::diagnostics::health::mark_component_error(
                    "scheduler",
                    format!("job {} failed", job.id),
                );
            }

            if let Err(e) = reschedule_after_run(&config, &job, success, &output) {
                crate::runtime::diagnostics::health::mark_component_error(
                    "scheduler",
                    e.to_string(),
                );
                tracing::warn!("Failed to persist scheduler run result: {e}");
            }
        }
    }
}

fn initialize_scheduler_state(config: &Config) {
    if let Err(error) = recover_interrupted_plan_jobs(config) {
        tracing::warn!(error = %error, "failed to recover interrupted plan executions");
    }
}

async fn execute_job_with_retry(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    let mut last_output = String::new();
    let retries = effective_retry_budget(config, job);
    let mut backoff_ms = config.reliability.provider_backoff_ms.max(200);

    for attempt in 0..=retries {
        let (success, output) = run_job_command(config, security, job).await;
        last_output = output;

        if success {
            return (true, last_output);
        }

        if last_output.starts_with("blocked by security policy:") {
            // Deterministic policy violations are not retryable.
            return (false, last_output);
        }

        if attempt < retries {
            let jitter_ms = u64::from(Utc::now().timestamp_subsec_millis() % 250);
            time::sleep(Duration::from_millis(backoff_ms + jitter_ms)).await;
            backoff_ms = (backoff_ms.saturating_mul(2)).min(30_000);
        }
    }

    (false, last_output)
}

fn effective_retry_budget(config: &Config, job: &CronJob) -> u32 {
    let retries = config.reliability.scheduler_retries;
    if job.origin == crate::platform::cron::CronJobOrigin::Agent {
        retries.min(job.max_attempts.saturating_sub(1))
    } else {
        retries
    }
}

fn is_env_assignment(word: &str) -> bool {
    word.contains('=')
        && word
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

fn strip_wrapping_quotes(token: &str) -> &str {
    token.trim_matches(|c| c == '"' || c == '\'')
}

fn forbidden_path_argument(security: &SecurityPolicy, command: &str) -> Option<String> {
    let mut normalized = command.to_string();
    for sep in ["&&", "||"] {
        normalized = normalized.replace(sep, "\x00");
    }
    for sep in ['\n', ';', '|'] {
        normalized = normalized.replace(sep, "\x00");
    }

    for segment in normalized.split('\x00') {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        // Skip leading env assignments and executable token.
        let mut idx = 0;
        while idx < tokens.len() && is_env_assignment(tokens[idx]) {
            idx += 1;
        }
        if idx >= tokens.len() {
            continue;
        }
        idx += 1;

        for token in &tokens[idx..] {
            let candidate = strip_wrapping_quotes(token);
            if candidate.is_empty() || candidate.starts_with('-') || candidate.contains("://") {
                continue;
            }

            let looks_like_path = candidate.starts_with('/')
                || candidate.starts_with("./")
                || candidate.starts_with("../")
                || candidate.starts_with("~/")
                || candidate.contains('/');

            if looks_like_path && !security.is_path_allowed(candidate) {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

fn policy_denial(route_marker: &str, reason: impl Into<String>) -> String {
    format!("{route_marker}\n{}", reason.into())
}

fn enforce_policy_invariants(
    security: &SecurityPolicy,
    command: &str,
    route_marker: &str,
) -> Result<(), String> {
    if !security.is_command_allowed(command) {
        return Err(policy_denial(
            route_marker,
            format!("blocked by security policy: command not allowed: {command}"),
        ));
    }

    if let Some(path) = forbidden_path_argument(security, command) {
        return Err(policy_denial(
            route_marker,
            format!("blocked by security policy: forbidden path argument: {path}"),
        ));
    }

    if let Err(policy_error) = security.consume_action_and_cost(0) {
        return Err(policy_denial(route_marker, policy_error));
    }

    Ok(())
}

async fn run_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    match job.origin {
        crate::platform::cron::CronJobOrigin::User => {
            run_user_job_command(config, security, job).await
        }
        crate::platform::cron::CronJobOrigin::Agent => {
            run_agent_job_command(config, security, job).await
        }
    }
}

pub async fn execute_job_once_for_integration(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    run_job_command(config, security, job).await
}

async fn run_user_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    if let Some(parsed) = parse_routed_job_command(&job.command) {
        return match parsed {
            ParsedRoutedJob::Ingestion(parsed) => {
                run_ingestion_job_command(config, security, parsed).await
            }
            ParsedRoutedJob::TrendAggregation(parsed) => {
                run_trend_aggregation_job_command(config, security, parsed).await
            }
            ParsedRoutedJob::XPoll(parsed) => {
                run_x_poll_job_command(config, security, parsed).await
            }
            ParsedRoutedJob::RssPoll(parsed) => {
                run_rss_poll_job_command(config, security, parsed).await
            }
        };
    }

    if let Err(output) = enforce_policy_invariants(security, &job.command, ROUTE_MARKER_USER_SHELL)
    {
        return (false, output);
    }

    let output = Command::new("sh")
        .arg("-lc")
        .arg(&job.command)
        .current_dir(&config.workspace_dir)
        .output()
        .await;

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!(
                "{ROUTE_MARKER_USER_SHELL}\nstatus={}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                stdout.trim(),
                stderr.trim()
            );
            (output.status.success(), combined)
        }
        Err(e) => (
            false,
            format!("{ROUTE_MARKER_USER_SHELL}\nspawn error: {e}"),
        ),
    }
}

#[derive(Debug, Clone)]
struct ParsedIngestionJob {
    source_kind: SourceKind,
    entity_id: String,
    source_ref: String,
    content: String,
}

#[derive(Debug, Clone)]
struct ParsedTrendAggregationJob {
    entity_id: String,
    topic_key: String,
    query: String,
}

#[derive(Debug, Clone)]
struct ParsedXPollJob {
    entity_id: String,
    query: String,
}

#[derive(Debug, Clone)]
struct ParsedRssPollJob {
    entity_id: String,
    url: String,
}

#[derive(Debug, Clone)]
enum ParsedRoutedJob {
    Ingestion(ParsedIngestionJob),
    TrendAggregation(ParsedTrendAggregationJob),
    XPoll(ParsedXPollJob),
    RssPoll(ParsedRssPollJob),
}

#[derive(Debug, Clone)]
struct RssPollItem {
    source_ref: String,
    content: String,
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

fn parse_routed_job_command(command: &str) -> Option<ParsedRoutedJob> {
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

fn resolve_x_bearer_token(raw: Option<String>) -> Result<String, String> {
    match raw {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(format!("{ROUTE_MARKER_X_POLL}\nmissing X_BEARER_TOKEN")),
    }
}

fn resolve_x_recent_search_endpoint(raw: Option<String>) -> String {
    match raw {
        Some(value) if !value.trim().is_empty() => value,
        _ => X_RECENT_SEARCH_ENDPOINT.to_string(),
    }
}

fn build_x_poll_envelopes(
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

fn parse_rss_items_from_xml(xml: &str, limit: usize) -> Vec<RssPollItem> {
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

fn build_rss_poll_envelopes(entity_id: &str, items: Vec<RssPollItem>) -> Vec<SignalEnvelope> {
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

async fn run_ingestion_job_command(
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

    if let Err(policy_error) = security.consume_action_and_cost(0) {
        return (
            false,
            format!("{ROUTE_MARKER_INGEST_PIPELINE}\nblocked by security policy: {policy_error}"),
        );
    }

    let memory = match create_memory(&config.memory, &config.workspace_dir, None) {
        Ok(memory) => memory,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_INGEST_PIPELINE}\ncreate_memory failed: {error}"),
            );
        }
    };
    let observer: Arc<dyn crate::runtime::observability::Observer> =
        Arc::from(create_observer(&config.observability));
    let pipeline = SqliteIngestionPipeline::new_with_observer(Arc::from(memory), observer);
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

async fn run_trend_aggregation_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedTrendAggregationJob,
) -> (bool, String) {
    if let Err(policy_error) = security.consume_action_and_cost(0) {
        return (
            false,
            format!("{ROUTE_MARKER_TREND_AGGREGATION}\nblocked by security policy: {policy_error}"),
        );
    }

    let memory = match create_memory(&config.memory, &config.workspace_dir, None) {
        Ok(memory) => memory,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_TREND_AGGREGATION}\ncreate_memory failed: {error}"),
            );
        }
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

async fn run_rss_poll_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedRssPollJob,
) -> (bool, String) {
    if let Err(policy_error) = security.consume_action_and_cost(0) {
        return (
            false,
            format!("{ROUTE_MARKER_RSS_POLL}\nblocked by security policy: {policy_error}"),
        );
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

    let memory = match create_memory(&config.memory, &config.workspace_dir, None) {
        Ok(memory) => memory,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_RSS_POLL}\ncreate_memory failed: {error}"),
            );
        }
    };
    let observer: Arc<dyn crate::runtime::observability::Observer> =
        Arc::from(create_observer(&config.observability));
    let pipeline = SqliteIngestionPipeline::new_with_observer(Arc::from(memory), observer);

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
async fn run_x_poll_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: ParsedXPollJob,
) -> (bool, String) {
    if let Err(policy_error) = security.consume_action_and_cost(0) {
        return (
            false,
            format!("{ROUTE_MARKER_X_POLL}\nblocked by security policy: {policy_error}"),
        );
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

    let memory = match create_memory(&config.memory, &config.workspace_dir, None) {
        Ok(memory) => memory,
        Err(error) => {
            return (
                false,
                format!("{ROUTE_MARKER_X_POLL}\ncreate_memory failed: {error}"),
            );
        }
    };
    let observer: Arc<dyn crate::runtime::observability::Observer> =
        Arc::from(create_observer(&config.observability));
    let pipeline = SqliteIngestionPipeline::new_with_observer(Arc::from(memory), observer);

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

#[allow(clippy::too_many_lines)]
async fn run_agent_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    if let Some(raw_plan) = job.command.strip_prefix("plan:") {
        if let Err(policy_error) = security.consume_action_and_cost(0) {
            return (
                false,
                format!("{ROUTE_MARKER_AGENT_PLANNER}\nblocked by security policy: {policy_error}"),
            );
        }

        let mut plan = match PlanParser::parse(raw_plan.trim()) {
            Ok(plan) => plan,
            Err(error) => {
                let _ = persist_plan_execution(config, job, "parse_failed", 1, 0, 1, 0, raw_plan);
                return (
                    false,
                    format!("{ROUTE_MARKER_AGENT_PLANNER}\nplan parse failed: {error}"),
                );
            }
        };

        let security_arc = Arc::new(security.clone());
        let mut registry = ToolRegistry::new(default_middleware_chain());
        for tool in default_tools(&security_arc) {
            registry.register(tool);
        }

        let runner = ToolStepRunner::new(
            Arc::new(registry),
            crate::core::tools::middleware::ExecutionContext::from_security(security_arc),
        );
        let execution_id = begin_plan_execution(config, job, &plan.id, raw_plan).ok();
        let max_attempts = job.max_attempts.max(1);
        let mut attempts = 1_u32;
        let mut final_report = match PlanExecutor::execute(&mut plan, &runner).await {
            Ok(report) => report,
            Err(error) => {
                if let Some(execution_id) = execution_id.as_deref() {
                    let _ = finalize_plan_execution(
                        config,
                        execution_id,
                        "execution_error",
                        attempts,
                        0,
                        1,
                        0,
                    );
                } else {
                    let _ = persist_plan_execution(
                        config,
                        job,
                        "execution_error",
                        attempts,
                        0,
                        1,
                        0,
                        raw_plan,
                    );
                }
                return (
                    false,
                    format!("{ROUTE_MARKER_AGENT_PLANNER}\nplan execution failed: {error}"),
                );
            }
        };

        while !final_report.success && attempts < max_attempts {
            attempts = attempts.saturating_add(1);
            let Ok(mut retry_plan) = PlanParser::parse(raw_plan.trim()) else {
                break;
            };
            let Ok(retry_report) = PlanExecutor::execute(&mut retry_plan, &runner).await else {
                break;
            };
            final_report = retry_report;
        }

        let success = final_report.success;
        let retry_limit_reached = !success && attempts >= max_attempts;
        let output = format!(
            "{ROUTE_MARKER_AGENT_PLANNER}\nsuccess={}\nattempts={attempts}\nmax_attempts={max_attempts}\nretry_limit_reached={retry_limit_reached}\ncompleted={}\nfailed={}\nskipped={}",
            final_report.success,
            final_report.completed_steps.len(),
            final_report.failed_steps.len(),
            final_report.skipped_steps.len()
        );
        let status = if final_report.success {
            "completed"
        } else {
            "failed"
        };
        if let Some(execution_id) = execution_id.as_deref() {
            let _ = finalize_plan_execution(
                config,
                execution_id,
                status,
                attempts,
                final_report.completed_steps.len(),
                final_report.failed_steps.len(),
                final_report.skipped_steps.len(),
            );
        } else {
            let _ = persist_plan_execution(
                config,
                job,
                status,
                attempts,
                final_report.completed_steps.len(),
                final_report.failed_steps.len(),
                final_report.skipped_steps.len(),
                raw_plan,
            );
        }
        return (success, output);
    }

    if let Err(output) =
        enforce_policy_invariants(security, &job.command, ROUTE_MARKER_AGENT_BLOCKED)
    {
        return (false, output);
    }

    (
        false,
        format!(
            "{ROUTE_MARKER_AGENT_BLOCKED}\nblocked by security policy: agent jobs cannot execute direct shell path"
        ),
    )
}

fn begin_plan_execution(
    config: &Config,
    job: &CronJob,
    plan_id: &str,
    plan_json: &str,
) -> anyhow::Result<String> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;
    let execution_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO plan_executions (
            id, job_id, plan_id, status, attempts,
            completed_steps, failed_steps, skipped_steps,
            plan_json, created_at
        ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, ?4, ?5)",
        params![execution_id, job.id, plan_id, plan_json, now],
    )?;
    Ok(execution_id)
}

fn finalize_plan_execution(
    config: &Config,
    execution_id: &str,
    status: &str,
    attempts: u32,
    completed_steps: usize,
    failed_steps: usize,
    skipped_steps: usize,
) -> anyhow::Result<()> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;
    conn.execute(
        "UPDATE plan_executions
         SET status = ?2,
             attempts = ?3,
             completed_steps = ?4,
             failed_steps = ?5,
             skipped_steps = ?6
         WHERE id = ?1",
        params![
            execution_id,
            status,
            i64::from(attempts),
            i64::try_from(completed_steps).unwrap_or(0),
            i64::try_from(failed_steps).unwrap_or(0),
            i64::try_from(skipped_steps).unwrap_or(0)
        ],
    )?;
    Ok(())
}

fn ensure_plan_execution_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plan_executions (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            plan_id TEXT,
            status TEXT NOT NULL,
            attempts INTEGER NOT NULL,
            completed_steps INTEGER NOT NULL,
            failed_steps INTEGER NOT NULL,
            skipped_steps INTEGER NOT NULL,
            plan_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_plan_executions_job ON plan_executions(job_id);
        CREATE INDEX IF NOT EXISTS idx_plan_executions_created_at ON plan_executions(created_at);",
    )?;
    Ok(())
}

fn ensure_cron_jobs_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cron_jobs (
            id          TEXT PRIMARY KEY,
            expression  TEXT NOT NULL,
            command     TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            next_run    TEXT NOT NULL,
            last_run    TEXT,
            last_status TEXT,
            last_output TEXT,
            job_kind    TEXT NOT NULL DEFAULT 'user',
            origin      TEXT NOT NULL DEFAULT 'user',
            expires_at  TEXT,
            max_attempts INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_cron_jobs_next_run ON cron_jobs(next_run);",
    )?;
    Ok(())
}

fn recover_interrupted_plan_jobs(config: &Config) -> anyhow::Result<usize> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;
    ensure_cron_jobs_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, job_id, plan_json FROM plan_executions WHERE status = 'running' ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let now = Utc::now().to_rfc3339();
    let mut recovered = 0_usize;
    for row in rows {
        let (execution_id, job_id, plan_json) = row?;
        let changed = conn.execute(
            "UPDATE cron_jobs
             SET next_run = ?1,
                 last_status = 'recover_pending',
                 last_output = 'recovered_from_plan_execution',
                 max_attempts = CASE WHEN max_attempts < 1 THEN 3 ELSE max_attempts END
             WHERE id = ?2 AND origin = 'agent'",
            params![now, job_id],
        )?;

        if changed == 0 {
            conn.execute(
                "INSERT INTO cron_jobs (
                    id, expression, command, created_at, next_run,
                    last_run, last_status, last_output,
                    job_kind, origin, expires_at, max_attempts
                ) VALUES (?1, '*/5 * * * *', ?2, ?3, ?4, NULL, 'recover_pending', 'recovered_from_plan_execution', 'agent', 'agent', NULL, 3)",
                params![Uuid::new_v4().to_string(), format!("plan:{plan_json}"), now, now],
            )?;
        }

        conn.execute(
            "UPDATE plan_executions SET status = 'requeued', attempts = CASE WHEN attempts < 1 THEN 1 ELSE attempts END WHERE id = ?1",
            params![execution_id],
        )?;
        recovered = recovered.saturating_add(1);
    }

    Ok(recovered)
}

#[allow(clippy::too_many_arguments)]
fn persist_plan_execution(
    config: &Config,
    job: &CronJob,
    status: &str,
    attempts: u32,
    completed_steps: usize,
    failed_steps: usize,
    skipped_steps: usize,
    plan_json: &str,
) -> anyhow::Result<()> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;

    let now = Utc::now().to_rfc3339();
    let execution_id = Uuid::new_v4().to_string();
    let plan_id = if let Ok(parsed) = PlanParser::parse(plan_json.trim()) {
        parsed.id
    } else {
        "unknown".to_string()
    };

    conn.execute(
        "INSERT INTO plan_executions (
            id, job_id, plan_id, status, attempts,
            completed_steps, failed_steps, skipped_steps,
            plan_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            execution_id,
            job.id,
            plan_id,
            status,
            i64::from(attempts),
            i64::try_from(completed_steps).unwrap_or(0),
            i64::try_from(failed_steps).unwrap_or(0),
            i64::try_from(skipped_steps).unwrap_or(0),
            plan_json,
            now
        ],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::platform::cron::{
        AGENT_PENDING_CAP, CronJobKind, CronJobMetadata, CronJobOrigin, add_job_with_metadata,
        due_jobs, list_jobs,
    };
    use crate::security::SecurityPolicy;
    use chrono::Duration as ChronoDuration;
    use rusqlite::Connection;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(tmp: &TempDir) -> Config {
        let config = Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        config
    }

    fn test_job(command: &str) -> CronJob {
        CronJob {
            id: "test-job".into(),
            expression: "* * * * *".into(),
            command: command.into(),
            next_run: Utc::now(),
            last_run: None,
            last_status: None,
            job_kind: CronJobKind::User,
            origin: CronJobOrigin::User,
            expires_at: None,
            max_attempts: 1,
        }
    }

    fn agent_metadata(expires_at: Option<chrono::DateTime<Utc>>) -> CronJobMetadata {
        CronJobMetadata {
            job_kind: CronJobKind::Agent,
            origin: CronJobOrigin::Agent,
            expires_at,
            max_attempts: 3,
        }
    }

    #[tokio::test]
    async fn run_job_command_success() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let job = test_job("echo scheduler-ok");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("scheduler-ok"));
        assert!(output.contains("status=exit status: 0"));
    }

    #[tokio::test]
    async fn run_job_command_ingest_api_success() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("ingest:api person:api.1 api:item-1 scheduler payload");

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=user-ingestion-pipeline"));
        assert!(output.contains("accepted=true"));
    }

    #[tokio::test]
    async fn run_job_command_ingest_api_rate_limits_same_source_ref() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("ingest:api person:api.rate api:item-rl scheduler payload");

        let (first_ok, first_output) = run_job_command(&config, &security, &job).await;
        assert!(first_ok, "{first_output}");
        assert!(first_output.contains("accepted=true"));

        let (second_ok, second_output) = run_job_command(&config, &security, &job).await;
        assert!(!second_ok, "{second_output}");
        assert!(second_output.contains("accepted=false"));
        assert!(second_output.contains("reason=rate_limited"));
    }

    #[tokio::test]
    async fn run_job_command_ingest_api_bad_format_falls_back_to_shell_and_fails() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("ingest:api missing-fields");

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
    }

    #[test]
    fn parse_routed_job_command_parses_trend_route() {
        let parsed = parse_routed_job_command("ingest:trend person:trend.1 release pulse signal")
            .expect("trend route should parse");
        match parsed {
            ParsedRoutedJob::TrendAggregation(job) => {
                assert_eq!(job.entity_id, "person:trend.1");
                assert_eq!(job.topic_key, "release");
                assert_eq!(job.query, "pulse signal");
            }
            ParsedRoutedJob::Ingestion(_) => panic!("expected trend route"),
            ParsedRoutedJob::XPoll(_) => panic!("expected trend route"),
            ParsedRoutedJob::RssPoll(_) => panic!("expected trend route"),
        }
    }

    #[test]
    fn parse_routed_job_command_normalizes_trend_topic_key() {
        let parsed = parse_routed_job_command(
            "ingest:trend person:trend.norm Release@@Topic/v2 pulse signal",
        )
        .expect("trend route should parse with normalized topic");
        match parsed {
            ParsedRoutedJob::TrendAggregation(job) => {
                assert_eq!(job.entity_id, "person:trend.norm");
                assert_eq!(job.topic_key, "release.topic.v2");
                assert_eq!(job.query, "pulse signal");
            }
            ParsedRoutedJob::Ingestion(_)
            | ParsedRoutedJob::XPoll(_)
            | ParsedRoutedJob::RssPoll(_) => panic!("expected trend route"),
        }
    }

    #[test]
    fn parse_routed_job_command_rejects_empty_normalized_trend_topic() {
        let parsed = parse_routed_job_command("ingest:trend person:trend.empty @@@ pulse signal");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_routed_job_command_rejects_trend_empty_query_after_trim() {
        let parsed = parse_routed_job_command("ingest:trend person:trend.empty release   ");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_routed_job_command_parses_x_route_with_prefixed_source_ref() {
        let parsed = parse_routed_job_command("ingest:x person:x.1 tweet-123 market pulse")
            .expect("x route should parse");
        match parsed {
            ParsedRoutedJob::Ingestion(job) => {
                assert_eq!(job.source_kind, SourceKind::Api);
                assert_eq!(job.entity_id, "person:x.1");
                assert_eq!(job.source_ref, "x:tweet-123");
                assert_eq!(job.content, "market pulse");
            }
            ParsedRoutedJob::TrendAggregation(_) => panic!("expected ingestion route"),
            ParsedRoutedJob::XPoll(_) => panic!("expected ingestion route"),
            ParsedRoutedJob::RssPoll(_) => panic!("expected ingestion route"),
        }
    }

    #[test]
    fn parse_routed_job_command_rejects_x_empty_content_after_trim() {
        let parsed = parse_routed_job_command("ingest:x person:x.empty tweet-1    ");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_routed_job_command_rejects_api_empty_content_after_trim() {
        let parsed = parse_routed_job_command("ingest:api person:api.empty api:item-1    ");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_routed_job_command_rejects_rss_empty_content_after_trim() {
        let parsed = parse_routed_job_command("ingest:rss person:rss.empty rss:item-1    ");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_routed_job_command_parses_x_poll_route() {
        let parsed =
            parse_routed_job_command("ingest:x-poll person:xpoll.1 rustlang from:rustlang")
                .expect("x poll route should parse");
        match parsed {
            ParsedRoutedJob::XPoll(job) => {
                assert_eq!(job.entity_id, "person:xpoll.1");
                assert_eq!(job.query, "rustlang from:rustlang");
            }
            ParsedRoutedJob::Ingestion(_)
            | ParsedRoutedJob::TrendAggregation(_)
            | ParsedRoutedJob::RssPoll(_) => {
                panic!("expected x poll route")
            }
        }
    }

    #[test]
    fn parse_routed_job_command_trims_x_poll_query_whitespace() {
        let parsed = parse_routed_job_command("ingest:x-poll person:xpoll.2   rustlang   ")
            .expect("x poll route should parse with trimmed query");
        match parsed {
            ParsedRoutedJob::XPoll(job) => {
                assert_eq!(job.entity_id, "person:xpoll.2");
                assert_eq!(job.query, "rustlang");
            }
            ParsedRoutedJob::Ingestion(_)
            | ParsedRoutedJob::TrendAggregation(_)
            | ParsedRoutedJob::RssPoll(_) => {
                panic!("expected x poll route")
            }
        }
    }

    #[test]
    fn parse_routed_job_command_rejects_x_poll_empty_query_after_trim() {
        let parsed = parse_routed_job_command("ingest:x-poll person:xpoll.3    ");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_routed_job_command_parses_rss_poll_route() {
        let parsed =
            parse_routed_job_command("ingest:rss-poll person:rss.1 https://example.test/feed.xml")
                .expect("rss poll route should parse");
        match parsed {
            ParsedRoutedJob::RssPoll(job) => {
                assert_eq!(job.entity_id, "person:rss.1");
                assert_eq!(job.url, "https://example.test/feed.xml");
            }
            ParsedRoutedJob::Ingestion(_)
            | ParsedRoutedJob::TrendAggregation(_)
            | ParsedRoutedJob::XPoll(_) => panic!("expected rss poll route"),
        }
    }

    #[test]
    fn parse_routed_job_command_rejects_rss_poll_empty_url_after_trim() {
        let parsed = parse_routed_job_command("ingest:rss-poll person:rss.2    ");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_rss_items_from_xml_extracts_entries() {
        let xml = r#"<?xml version='1.0'?>
            <rss><channel>
                <item>
                    <title><![CDATA[Release A]]></title>
                    <description>Alpha details</description>
                    <guid>id-a</guid>
                </item>
                <item>
                    <title>Release B</title>
                    <link>https://example.test/b</link>
                </item>
            </channel></rss>"#;

        let items = parse_rss_items_from_xml(xml, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].source_ref, "rss:id-a");
        assert!(items[0].content.contains("Release A"));
        assert!(items[0].content.contains("Alpha details"));
        assert_eq!(items[1].source_ref, "rss:https://example.test/b");
    }

    #[test]
    fn parse_rss_items_from_xml_respects_limit_and_skips_empty_items() {
        let xml = r#"<?xml version='1.0'?>
            <rss><channel>
                <item>
                    <title>Release A</title>
                    <description>Alpha details</description>
                    <guid>id-a</guid>
                </item>
                <item>
                    <title><![CDATA[   ]]></title>
                    <description><![CDATA[   ]]></description>
                    <guid>id-empty</guid>
                </item>
                <item>
                    <title>Release B</title>
                    <description>Beta details</description>
                    <guid>id-b</guid>
                </item>
            </channel></rss>"#;

        let items = parse_rss_items_from_xml(xml, 1);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source_ref, "rss:id-a");

        let items = parse_rss_items_from_xml(xml, 10);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].source_ref, "rss:id-a");
        assert_eq!(items[1].source_ref, "rss:id-b");
    }

    #[test]
    fn build_rss_poll_envelopes_marks_news_source() {
        let items = vec![RssPollItem {
            source_ref: "rss:id-a".to_string(),
            content: "Release A - Alpha details".to_string(),
        }];
        let envelopes = build_rss_poll_envelopes("person:rss.meta", items);
        assert_eq!(envelopes.len(), 1);
        let envelope = &envelopes[0];
        assert_eq!(envelope.source_kind, SourceKind::News);
        assert_eq!(envelope.entity_id, "person:rss.meta");
        assert_eq!(envelope.source_ref, "rss:id-a");
    }

    #[test]
    fn resolve_x_bearer_token_rejects_missing_or_empty() {
        let missing = resolve_x_bearer_token(None).expect_err("missing token should fail");
        assert!(missing.contains("missing X_BEARER_TOKEN"));

        let empty =
            resolve_x_bearer_token(Some("   ".to_string())).expect_err("empty token should fail");
        assert!(empty.contains("missing X_BEARER_TOKEN"));

        let valid = resolve_x_bearer_token(Some("token-123".to_string())).expect("valid token");
        assert_eq!(valid, "token-123");
    }

    #[test]
    fn resolve_x_recent_search_endpoint_defaults_when_unset() {
        assert_eq!(
            resolve_x_recent_search_endpoint(None),
            X_RECENT_SEARCH_ENDPOINT.to_string()
        );
        assert_eq!(
            resolve_x_recent_search_endpoint(Some("".to_string())),
            X_RECENT_SEARCH_ENDPOINT.to_string()
        );
        assert_eq!(
            resolve_x_recent_search_endpoint(Some("https://example.test/x".to_string())),
            "https://example.test/x".to_string()
        );
    }

    #[test]
    fn build_x_poll_envelopes_maps_metadata_and_prefix() {
        let tweets = vec![XRecentTweet {
            id: "999".to_string(),
            text: "hello from x".to_string(),
            lang: Some("en".to_string()),
            author_id: Some("author-1".to_string()),
        }];
        let envelopes = build_x_poll_envelopes("person:xpoll.meta", "rust lang", tweets);
        assert_eq!(envelopes.len(), 1);
        let envelope = &envelopes[0];
        assert_eq!(envelope.source_ref, "x:999");
        assert_eq!(envelope.entity_id, "person:xpoll.meta");
        assert_eq!(envelope.language.as_deref(), Some("en"));
        assert_eq!(
            envelope.metadata.get("x_author_id").map(String::as_str),
            Some("author-1")
        );
        assert_eq!(
            envelope.metadata.get("x_query").map(String::as_str),
            Some("rust lang")
        );
    }

    #[tokio::test]
    async fn run_job_command_ingest_x_success() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("ingest:x person:x.1 tweet-xyz scheduler x payload");

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=user-ingestion-pipeline"));
        assert!(output.contains("accepted=true"));
        assert!(output.contains("external.api.x:tweet-xyz") || output.contains("slot_key="));
    }

    #[tokio::test]
    async fn run_job_command_ingest_rss_poll_success_with_mock_feed() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let server = MockServer::start().await;
        let body = r#"<?xml version='1.0'?>
            <rss><channel>
                <item><title>Release A</title><description>Alpha</description><guid>id-a</guid></item>
                <item><title>Release B</title><description>Beta</description><guid>id-b</guid></item>
            </channel></rss>"#;
        Mock::given(method("GET"))
            .and(path("/feed.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let job = test_job(&format!(
            "ingest:rss-poll person:rss.poll {}",
            server.uri() + "/feed.xml"
        ));
        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=user-rss-poll"));
        assert!(output.contains("accepted=true"));
        assert!(output.contains("accepted_count="));
    }

    #[tokio::test]
    async fn run_job_command_ingest_rss_poll_empty_feed_returns_no_items() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let server = MockServer::start().await;
        let body = "<?xml version='1.0'?><rss><channel></channel></rss>";
        Mock::given(method("GET"))
            .and(path("/empty.xml"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let job = test_job(&format!(
            "ingest:rss-poll person:rss.empty {}",
            server.uri() + "/empty.xml"
        ));
        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=user-rss-poll"));
        assert!(output.contains("accepted=false"));
        assert!(output.contains("reason=no_items"));
    }

    #[tokio::test]
    async fn run_job_command_ingest_rss_poll_invalid_url_fails() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("ingest:rss-poll person:rss.bad not-a-valid-url");

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success, "{output}");
        assert!(output.contains("route=user-rss-poll"));
        assert!(output.contains("request failed"));
    }

    #[tokio::test]
    async fn run_job_command_ingest_trend_writes_snapshot_slot() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let memory =
            create_memory(&config.memory, &config.workspace_dir, None).expect("sqlite memory");
        memory
            .append_event(MemoryEventInput::new(
                "person:trend.1",
                "external.api.api-item-1",
                MemoryEventType::FactAdded,
                "release pulse signal",
                MemorySource::ExternalSecondary,
                PrivacyLevel::Private,
            ))
            .await
            .expect("seed api event");
        memory
            .append_event(MemoryEventInput::new(
                "person:trend.1",
                "external.news.rss-item-1",
                MemoryEventType::FactAdded,
                "release pulse signal from feed",
                MemorySource::ExternalSecondary,
                PrivacyLevel::Private,
            ))
            .await
            .expect("seed rss event");

        let trend_job = test_job("ingest:trend person:trend.1 release release pulse");
        let (trend_ok, trend_out) = run_job_command(&config, &security, &trend_job).await;
        assert!(trend_ok, "{trend_out}");
        assert!(trend_out.contains("route=user-trend-aggregation"));
        assert!(trend_out.contains("accepted=true"));
        assert!(trend_out.contains("slot_key=trend.snapshot.release"));

        let memory = create_memory(&config.memory, &config.workspace_dir, None)
            .expect("sqlite memory should open");
        let slot = memory
            .resolve_slot("person:trend.1", "trend.snapshot.release")
            .await
            .expect("resolve trend snapshot should succeed")
            .expect("trend snapshot should exist");
        assert!(slot.value.contains("trend topic=release"));
        assert!(slot.value.contains("external."));
    }

    #[tokio::test]
    async fn run_job_command_failure() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let job = test_job("ls definitely_missing_file_for_scheduler_test");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("definitely_missing_file_for_scheduler_test"));
        assert!(output.contains("status=exit status:"));
    }

    #[tokio::test]
    async fn run_job_command_blocks_disallowed_command() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["echo".into()];
        let job = test_job("curl https://evil.example");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("blocked by security policy"));
        assert!(output.contains("command not allowed"));
    }

    #[tokio::test]
    async fn run_job_command_blocks_forbidden_path_argument() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["cat".into()];
        let job = test_job("cat /etc/passwd");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("blocked by security policy"));
        assert!(output.contains("forbidden path argument"));
        assert!(output.contains("/etc/passwd"));
    }

    #[tokio::test]
    async fn execute_job_with_retry_recovers_after_first_failure() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.reliability.scheduler_retries = 1;
        config.reliability.provider_backoff_ms = 1;
        config.autonomy.allowed_commands = vec!["sh".into()];
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        std::fs::write(
            config.workspace_dir.join("retry-once.sh"),
            "#!/bin/sh\nif [ -f retry-ok.flag ]; then\n  echo recovered\n  exit 0\nfi\ntouch retry-ok.flag\nexit 1\n",
        )
        .unwrap();
        let job = test_job("sh ./retry-once.sh");

        let (success, output) = execute_job_with_retry(&config, &security, &job).await;
        assert!(success);
        assert!(output.contains("recovered"));
    }

    #[tokio::test]
    async fn execute_job_with_retry_exhausts_attempts() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.reliability.scheduler_retries = 1;
        config.reliability.provider_backoff_ms = 1;
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let job = test_job("ls always_missing_for_retry_test");

        let (success, output) = execute_job_with_retry(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("always_missing_for_retry_test"));
    }

    #[tokio::test]
    async fn run_job_command_policy_blocks_when_action_limit_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.max_actions_per_hour = 0;
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("echo should-not-run");

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("action limit"));
    }

    #[tokio::test]
    async fn scheduler_agent_jobs_never_direct_shell() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["sh".into()];
        let marker_file = "agent-shell-marker.txt";
        let marker_path = config.workspace_dir.join(marker_file);
        let command = format!("sh -c 'touch {marker_file}'");
        let mut job = test_job(&command);
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert_eq!(
            output,
            "route=agent-no-direct-shell\nblocked by security policy: agent jobs cannot execute direct shell path"
        );
        assert!(!marker_path.exists());
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_executes_via_planner() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let plan_json = r#"{"id":"agent-plan-1","description":"agent plan","steps":[{"id":"s1","description":"checkpoint","action":{"kind":"checkpoint","label":"gate"},"depends_on":[]},{"id":"s2","description":"prompt","action":{"kind":"prompt","text":"done"},"depends_on":["s1"]}]}"#;
        let mut job = test_job(&format!("plan:{plan_json}"));
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=agent-planner"));
        assert!(output.contains("success=true"));
        assert!(output.contains("attempts=1"));
        assert!(output.contains("retry_limit_reached=false"));
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_rejects_invalid_plan() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let mut job = test_job("plan:{\"id\":\"broken\",\"description\":\"x\",\"steps\":[]}");
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success, "{output}");
        assert!(output.contains("route=agent-planner"));
        assert!(output.contains("plan parse failed"));
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_retries_failed_execution_once() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let plan_json = r#"{"id":"agent-plan-fail","description":"agent plan fail","steps":[{"id":"s1","description":"missing tool","action":{"kind":"tool_call","tool_name":"nonexistent","args":{}},"depends_on":[]}]}"#;
        let mut job = test_job(&format!("plan:{plan_json}"));
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;
        job.max_attempts = 2;

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success, "{output}");
        assert!(output.contains("route=agent-planner"));
        assert!(output.contains("attempts=2"));
        assert!(output.contains("max_attempts=2"));
        assert!(output.contains("retry_limit_reached=true"));
        assert!(output.contains("success=false"));
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_respects_single_attempt_budget() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let plan_json = r#"{"id":"agent-plan-fail-1","description":"agent plan fail","steps":[{"id":"s1","description":"missing tool","action":{"kind":"tool_call","tool_name":"nonexistent","args":{}},"depends_on":[]}]}"#;
        let mut job = test_job(&format!("plan:{plan_json}"));
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;
        job.max_attempts = 1;

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success, "{output}");
        assert!(output.contains("route=agent-planner"));
        assert!(output.contains("attempts=1"));
        assert!(output.contains("max_attempts=1"));
        assert!(output.contains("retry_limit_reached=true"));
        assert!(output.contains("success=false"));
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_clamps_zero_attempt_budget_to_one() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let plan_json = r#"{"id":"agent-plan-fail-0","description":"agent plan fail","steps":[{"id":"s1","description":"missing tool","action":{"kind":"tool_call","tool_name":"nonexistent","args":{}},"depends_on":[]}]}"#;
        let mut job = test_job(&format!("plan:{plan_json}"));
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;
        job.max_attempts = 0;

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success, "{output}");
        assert!(output.contains("route=agent-planner"));
        assert!(output.contains("attempts=1"));
        assert!(output.contains("max_attempts=1"));
        assert!(output.contains("retry_limit_reached=true"));
        assert!(output.contains("success=false"));
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_persists_execution_row() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let plan_json = r#"{"id":"agent-plan-persist","description":"persist","steps":[{"id":"s1","description":"checkpoint","action":{"kind":"checkpoint","label":"gate"},"depends_on":[]}]}"#;
        let mut job = test_job(&format!("plan:{plan_json}"));
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");

        let db_path = config.workspace_dir.join("cron").join("jobs.db");
        let conn = Connection::open(db_path).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM plan_executions WHERE job_id = ?1 AND plan_id = ?2",
                rusqlite::params![job.id, "agent-plan-persist"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn scheduler_agent_plan_route_persists_parse_failure_row() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let mut job = test_job("plan:{\"id\":\"broken\",\"description\":\"x\",\"steps\":[]}");
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;

        let (success, _output) = run_job_command(&config, &security, &job).await;
        assert!(!success);

        let db_path = config.workspace_dir.join("cron").join("jobs.db");
        let conn = Connection::open(db_path).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM plan_executions WHERE job_id = ?1 ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![job.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "parse_failed");
    }

    #[test]
    fn recover_interrupted_plan_jobs_requeues_running_execution() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let db_path = config.workspace_dir.join("cron").join("jobs.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        ensure_plan_execution_schema(&conn).unwrap();
        ensure_cron_jobs_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO plan_executions (
                id, job_id, plan_id, status, attempts,
                completed_steps, failed_steps, skipped_steps,
                plan_json, created_at
            ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, ?4, ?5)",
            rusqlite::params![
                "exec-running-1",
                "job-missing-1",
                "plan-recover-1",
                "{\"id\":\"plan-recover-1\",\"description\":\"r\",\"steps\":[{\"id\":\"s1\",\"description\":\"c\",\"action\":{\"kind\":\"checkpoint\",\"label\":\"x\"},\"depends_on\":[]}]}",
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        drop(conn);

        let recovered = recover_interrupted_plan_jobs(&config).unwrap();
        assert_eq!(recovered, 1);

        let jobs = list_jobs(&config).unwrap();
        assert!(jobs.iter().any(|job| {
            job.origin == CronJobOrigin::Agent
                && job.command.starts_with("plan:")
                && job.command.contains("plan-recover-1")
        }));

        let recovered_job = jobs
            .iter()
            .find(|job| {
                job.origin == CronJobOrigin::Agent && job.command.contains("plan-recover-1")
            })
            .expect("recovered agent plan job should exist");
        assert_eq!(recovered_job.max_attempts, 3);
        assert_eq!(recovered_job.job_kind, CronJobKind::Agent);

        let conn = Connection::open(&db_path).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM plan_executions WHERE id = 'exec-running-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "requeued");
    }

    #[test]
    fn initialize_scheduler_state_runs_recovery_path() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let db_path = config.workspace_dir.join("cron").join("jobs.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        ensure_plan_execution_schema(&conn).unwrap();
        ensure_cron_jobs_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO plan_executions (
                id, job_id, plan_id, status, attempts,
                completed_steps, failed_steps, skipped_steps,
                plan_json, created_at
            ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, ?4, ?5)",
            rusqlite::params![
                "exec-running-init",
                "job-missing-init",
                "plan-recover-init",
                "{\"id\":\"plan-recover-init\",\"description\":\"r\",\"steps\":[{\"id\":\"s1\",\"description\":\"c\",\"action\":{\"kind\":\"checkpoint\",\"label\":\"x\"},\"depends_on\":[]}]}",
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        drop(conn);

        initialize_scheduler_state(&config);

        let conn = Connection::open(&db_path).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM plan_executions WHERE id = 'exec-running-init'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "requeued");
    }

    #[test]
    fn recover_interrupted_plan_jobs_updates_existing_agent_job_in_place() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let db_path = config.workspace_dir.join("cron").join("jobs.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        ensure_plan_execution_schema(&conn).unwrap();
        ensure_cron_jobs_schema(&conn).unwrap();

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO cron_jobs (
                id, expression, command, created_at, next_run,
                last_run, last_status, last_output,
                job_kind, origin, expires_at, max_attempts
            ) VALUES (?1, '*/5 * * * *', ?2, ?3, ?4, NULL, NULL, NULL, 'agent', 'agent', NULL, 3)",
            rusqlite::params![
                "job-existing-1",
                "plan:{\"id\":\"plan-existing\",\"description\":\"d\",\"steps\":[{\"id\":\"s1\",\"description\":\"c\",\"action\":{\"kind\":\"checkpoint\",\"label\":\"x\"},\"depends_on\":[]}]}".to_string(),
                now,
                now
            ],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO plan_executions (
                id, job_id, plan_id, status, attempts,
                completed_steps, failed_steps, skipped_steps,
                plan_json, created_at
            ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, ?4, ?5)",
            rusqlite::params![
                "exec-running-existing",
                "job-existing-1",
                "plan-existing",
                "{\"id\":\"plan-existing\",\"description\":\"d\",\"steps\":[{\"id\":\"s1\",\"description\":\"c\",\"action\":{\"kind\":\"checkpoint\",\"label\":\"x\"},\"depends_on\":[]}]}",
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        drop(conn);

        let recovered = recover_interrupted_plan_jobs(&config).unwrap();
        assert_eq!(recovered, 1);

        let conn = Connection::open(&db_path).unwrap();
        let cron_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cron_jobs WHERE origin = 'agent'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            cron_count, 1,
            "recovery should not duplicate existing agent job"
        );

        let status: String = conn
            .query_row(
                "SELECT status FROM plan_executions WHERE id = 'exec-running-existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "requeued");

        let attempts: i64 = conn
            .query_row(
                "SELECT attempts FROM plan_executions WHERE id = 'exec-running-existing'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attempts, 1);

        let last_status: String = conn
            .query_row(
                "SELECT last_status FROM cron_jobs WHERE id = 'job-existing-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(last_status, "recover_pending");
    }

    #[test]
    fn recover_interrupted_plan_jobs_normalizes_existing_job_max_attempts() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let db_path = config.workspace_dir.join("cron").join("jobs.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(&db_path).unwrap();
        ensure_plan_execution_schema(&conn).unwrap();
        ensure_cron_jobs_schema(&conn).unwrap();

        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO cron_jobs (
                id, expression, command, created_at, next_run,
                last_run, last_status, last_output,
                job_kind, origin, expires_at, max_attempts
            ) VALUES (?1, '*/5 * * * *', ?2, ?3, ?4, NULL, NULL, NULL, 'agent', 'agent', NULL, 0)",
            rusqlite::params![
                "job-existing-zero-attempts",
                "plan:{\"id\":\"plan-existing-zero\",\"description\":\"d\",\"steps\":[{\"id\":\"s1\",\"description\":\"c\",\"action\":{\"kind\":\"checkpoint\",\"label\":\"x\"},\"depends_on\":[]}]}".to_string(),
                now,
                now
            ],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO plan_executions (
                id, job_id, plan_id, status, attempts,
                completed_steps, failed_steps, skipped_steps,
                plan_json, created_at
            ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, ?4, ?5)",
            rusqlite::params![
                "exec-running-existing-zero",
                "job-existing-zero-attempts",
                "plan-existing-zero",
                "{\"id\":\"plan-existing-zero\",\"description\":\"d\",\"steps\":[{\"id\":\"s1\",\"description\":\"c\",\"action\":{\"kind\":\"checkpoint\",\"label\":\"x\"},\"depends_on\":[]}]}",
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
        drop(conn);

        let recovered = recover_interrupted_plan_jobs(&config).unwrap();
        assert_eq!(recovered, 1);

        let conn = Connection::open(&db_path).unwrap();
        let max_attempts: i64 = conn
            .query_row(
                "SELECT max_attempts FROM cron_jobs WHERE id = 'job-existing-zero-attempts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(max_attempts, 3);
    }

    #[tokio::test]
    async fn scheduler_user_jobs_still_execute_expected_path() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["sh".into()];
        let marker_file = "user-shell-marker.txt";
        let marker_path = config.workspace_dir.join(marker_file);
        let command = format!("sh -c 'touch {marker_file}'");
        let job = test_job(&command);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("status=exit status: 0"));
        assert!(marker_path.exists());
    }

    #[test]
    fn scheduler_agent_queue_bounded() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let expires_at = Some(Utc::now() + ChronoDuration::hours(1));

        for idx in 0..AGENT_PENDING_CAP {
            let command = format!("echo queue-{idx}");
            add_job_with_metadata(
                &config,
                "*/5 * * * *",
                &command,
                &agent_metadata(expires_at),
            )
            .unwrap();
        }

        let err = add_job_with_metadata(
            &config,
            "*/5 * * * *",
            "echo overflow",
            &agent_metadata(expires_at),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("agent-origin queue cap reached (5 pending jobs)")
        );
    }

    #[test]
    fn scheduler_expires_agent_jobs() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let expired_at = Some(Utc::now() - ChronoDuration::minutes(1));

        let _job = add_job_with_metadata(
            &config,
            "*/5 * * * *",
            "echo expired",
            &agent_metadata(expired_at),
        )
        .unwrap();

        let jobs = due_jobs(&config, Utc::now()).unwrap();
        assert!(jobs.is_empty());

        let remaining = list_jobs(&config).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn scheduler_agent_retry_budget_is_bounded_by_max_attempts() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.reliability.scheduler_retries = 9;

        let mut job = test_job("echo bounded-retries");
        job.origin = CronJobOrigin::Agent;
        job.max_attempts = 3;

        assert_eq!(effective_retry_budget(&config, &job), 2);

        job.max_attempts = 1;
        assert_eq!(effective_retry_budget(&config, &job), 0);
    }
}
