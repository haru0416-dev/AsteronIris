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
    let parsed =
        parse_routed_job_command("ingest:trend person:trend.norm Release@@Topic/v2 pulse signal")
            .expect("trend route should parse with normalized topic");
    match parsed {
        ParsedRoutedJob::TrendAggregation(job) => {
            assert_eq!(job.entity_id, "person:trend.norm");
            assert_eq!(job.topic_key, "release.topic.v2");
            assert_eq!(job.query, "pulse signal");
        }
        ParsedRoutedJob::Ingestion(_) | ParsedRoutedJob::XPoll(_) | ParsedRoutedJob::RssPoll(_) => {
            panic!("expected trend route")
        }
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
    let parsed = parse_routed_job_command("ingest:x-poll person:xpoll.1 rustlang from:rustlang")
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

    let memory = create_memory(&config.memory, &config.workspace_dir, None).expect("sqlite memory");
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
        .find(|job| job.origin == CronJobOrigin::Agent && job.command.contains("plan-recover-1"))
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
