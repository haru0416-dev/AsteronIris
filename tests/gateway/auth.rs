use asteroniris::config::{Config, GatewayDefenseMode, ObservabilityConfig, WebhookConfig};
use asteroniris::gateway::run_gateway_with_listener;
use asteroniris::observability::create_observer;
use asteroniris::observability::multi::MultiObserver;
use asteroniris::observability::traits::{Observer, ObserverEvent, ObserverMetric};
use asteroniris::security::pairing::PairingGuard;
use reqwest::StatusCode;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tempfile::TempDir;

struct GatewayTestServer {
    port: u16,
    handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl GatewayTestServer {
    #[allow(clippy::field_reassign_with_default)]
    async fn start(
        require_pairing: bool,
        paired_tokens: Vec<String>,
        webhook_secret: &str,
        defense_mode: GatewayDefenseMode,
        defense_kill_switch: bool,
    ) -> Self {
        let workspace = TempDir::new().expect("temp workspace should be created");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral gateway listener should bind");
        let port = listener
            .local_addr()
            .expect("ephemeral gateway listener should expose local address")
            .port();

        let mut config = Config::default();
        config.workspace_dir = workspace.path().to_path_buf();
        config.config_path = workspace.path().join("config.toml");
        config.default_provider = Some("openrouter".to_string());
        config.api_key = Some("sk-test-key".to_string());
        config.memory.backend = "none".to_string();
        config.memory.auto_save = false;
        config.gateway.require_pairing = require_pairing;
        config.gateway.paired_tokens = paired_tokens;
        config.gateway.defense_mode = defense_mode;
        config.gateway.defense_kill_switch = defense_kill_switch;
        config.channels_config.webhook = Some(WebhookConfig {
            port,
            secret: Some(webhook_secret.to_string()),
        });

        let host = "127.0.0.1".to_string();
        let handle =
            tokio::spawn(async move { run_gateway_with_listener(&host, listener, config).await });

        wait_until_gateway_ready(port).await;

        Self { port, handle }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }
}

impl Drop for GatewayTestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn wait_until_gateway_ready(port: u16) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .expect("reqwest client should be built");

    for _ in 0..80 {
        let health = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await;
        if matches!(health, Ok(resp) if resp.status() == StatusCode::OK) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("gateway did not become ready on port {port}");
}

#[tokio::test]
async fn gateway_auth_enforces_deny_and_allows_authorized_path() {
    let server = GatewayTestServer::start(
        true,
        vec!["token-abc".to_string()],
        "gateway-shared-secret",
        GatewayDefenseMode::Enforce,
        false,
    )
    .await;
    let client = reqwest::Client::new();

    let no_bearer = client
        .post(server.url("/webhook"))
        .header("X-Webhook-Secret", "gateway-shared-secret")
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("request without bearer should complete");
    assert_eq!(no_bearer.status(), StatusCode::UNAUTHORIZED);
    let no_bearer_body: Value = no_bearer
        .json()
        .await
        .expect("unauthorized response should be json");
    assert!(
        no_bearer_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("pair first"))
    );

    let no_secret = client
        .post(server.url("/webhook"))
        .header("Authorization", "Bearer token-abc")
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("request without webhook secret should complete");
    assert_eq!(no_secret.status(), StatusCode::UNAUTHORIZED);
    let no_secret_body: Value = no_secret
        .json()
        .await
        .expect("invalid secret response should be json");
    assert!(
        no_secret_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("X-Webhook-Secret"))
    );

    let authorized_bad_json = client
        .post(server.url("/webhook"))
        .header("Authorization", "Bearer token-abc")
        .header("X-Webhook-Secret", "gateway-shared-secret")
        .json(&serde_json::json!({"persona_state": "missing message"}))
        .send()
        .await
        .expect("authorized request should complete");
    assert_eq!(authorized_bad_json.status(), StatusCode::BAD_REQUEST);
    let authorized_bad_json_body: Value = authorized_bad_json
        .json()
        .await
        .expect("bad json response should be json");
    assert!(
        authorized_bad_json_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("Invalid JSON"))
    );
}

#[tokio::test]
async fn gateway_audit_mode_records_violations_without_blocking() {
    let server = GatewayTestServer::start(
        true,
        vec!["token-abc".to_string()],
        "gateway-shared-secret",
        GatewayDefenseMode::Audit,
        false,
    )
    .await;
    let client = reqwest::Client::new();

    let response = client
        .post(server.url("/webhook"))
        .json(&serde_json::json!({"persona_state": "missing message"}))
        .send()
        .await
        .expect("audit mode request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = response
        .json()
        .await
        .expect("audit mode response should be json");
    assert!(
        body.get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("Invalid JSON"))
    );
}

#[tokio::test]
async fn gateway_warn_mode_returns_warning_path_without_deny() {
    let server = GatewayTestServer::start(
        true,
        vec!["token-abc".to_string()],
        "gateway-shared-secret",
        GatewayDefenseMode::Warn,
        false,
    )
    .await;
    let client = reqwest::Client::new();

    let response = client
        .post(server.url("/webhook"))
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("warn mode request should complete");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body: Value = response
        .json()
        .await
        .expect("warn mode response should be json");
    assert_eq!(body.get("mode"), Some(&Value::String("warn".to_string())));
    assert_eq!(
        body.get("warning"),
        Some(&Value::String("missing_or_invalid_bearer".to_string()))
    );
    assert_eq!(body.get("blocked"), Some(&Value::Bool(false)));
}

#[tokio::test]
async fn gateway_kill_switch_forces_non_blocking_path() {
    let server = GatewayTestServer::start(
        true,
        vec!["token-abc".to_string()],
        "gateway-shared-secret",
        GatewayDefenseMode::Enforce,
        true,
    )
    .await;
    let client = reqwest::Client::new();

    let response = client
        .post(server.url("/webhook"))
        .json(&serde_json::json!({"persona_state": "missing message"}))
        .send()
        .await
        .expect("kill-switch mode request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value = response
        .json()
        .await
        .expect("kill-switch response should be json");
    assert!(
        body.get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("Invalid JSON"))
    );
}

#[test]
fn pairing_lifecycle_covers_success_failure_and_token_authentication() {
    let guard = PairingGuard::new(true, &[]);
    let code = guard
        .pairing_code()
        .expect("pairing code should exist with pairing enabled and no tokens");

    let invalid = guard
        .try_pair("not-a-valid-code")
        .expect("invalid pairing code should not lock out on first attempt");
    assert!(invalid.is_none());

    let token = guard
        .try_pair(&code)
        .expect("valid pairing code should pair successfully")
        .expect("pairing should return a bearer token");
    assert!(token.starts_with("zc_"));
    assert!(guard.is_authenticated(&token));
    assert!(
        guard.pairing_code().is_none(),
        "pairing code should be consumed"
    );

    let reuse = guard
        .try_pair(&code)
        .expect("reusing a consumed pairing code should not lock out immediately");
    assert!(
        reuse.is_none(),
        "consumed pairing code must not produce another bearer token"
    );
}

#[tokio::test]
async fn gateway_pair_endpoint_enforces_retry_after_lockout() {
    let server = GatewayTestServer::start(
        true,
        vec![],
        "gateway-shared-secret",
        GatewayDefenseMode::Enforce,
        false,
    )
    .await;
    let client = reqwest::Client::new();

    for _ in 0..5 {
        let denied = client
            .post(server.url("/pair"))
            .header("X-Pairing-Code", "not-a-valid-code")
            .send()
            .await
            .expect("invalid pairing request should complete");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let denied_body: Value = denied
            .json()
            .await
            .expect("invalid pairing denial should be json");
        assert_eq!(
            denied_body.get("error").and_then(Value::as_str),
            Some("Invalid pairing code")
        );
    }

    let locked = client
        .post(server.url("/pair"))
        .header("X-Pairing-Code", "not-a-valid-code")
        .send()
        .await
        .expect("lockout pairing request should complete");
    assert_eq!(locked.status(), StatusCode::TOO_MANY_REQUESTS);
    let lockout_body: Value = locked
        .json()
        .await
        .expect("lockout response should be json");
    assert!(
        lockout_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("Too many failed attempts"))
    );
    let retry_after = lockout_body
        .get("retry_after")
        .and_then(Value::as_u64)
        .expect("lockout response should include retry_after");
    assert!(retry_after > 0 && retry_after <= 300);
}

#[tokio::test]
async fn gateway_webhook_denies_missing_invalid_and_mismatched_auth_paths() {
    let server = GatewayTestServer::start(
        true,
        vec!["token-abc".to_string()],
        "gateway-shared-secret",
        GatewayDefenseMode::Enforce,
        false,
    )
    .await;
    let client = reqwest::Client::new();

    let missing_token = client
        .post(server.url("/webhook"))
        .header("X-Webhook-Secret", "gateway-shared-secret")
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("request without bearer should complete");
    assert_eq!(missing_token.status(), StatusCode::UNAUTHORIZED);
    let missing_token_body: Value = missing_token
        .json()
        .await
        .expect("missing-token response should be json");
    assert!(
        missing_token_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("pair first"))
    );

    let invalid_token = client
        .post(server.url("/webhook"))
        .header("Authorization", "Bearer token-invalid")
        .header("X-Webhook-Secret", "gateway-shared-secret")
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("request with invalid bearer should complete");
    assert_eq!(invalid_token.status(), StatusCode::UNAUTHORIZED);
    let invalid_token_body: Value = invalid_token
        .json()
        .await
        .expect("invalid-token response should be json");
    assert!(
        invalid_token_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("pair first"))
    );

    let missing_secret = client
        .post(server.url("/webhook"))
        .header("Authorization", "Bearer token-abc")
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("request without secret should complete");
    assert_eq!(missing_secret.status(), StatusCode::UNAUTHORIZED);
    let missing_secret_body: Value = missing_secret
        .json()
        .await
        .expect("missing-secret response should be json");
    assert!(
        missing_secret_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("X-Webhook-Secret"))
    );

    let mismatched_secret = client
        .post(server.url("/webhook"))
        .header("Authorization", "Bearer token-abc")
        .header("X-Webhook-Secret", "gateway-shared-secret-mismatch")
        .json(&serde_json::json!({"message": "hello"}))
        .send()
        .await
        .expect("request with mismatched secret should complete");
    assert_eq!(mismatched_secret.status(), StatusCode::UNAUTHORIZED);
    let mismatched_secret_body: Value = mismatched_secret
        .json()
        .await
        .expect("mismatched-secret response should be json");
    assert!(
        mismatched_secret_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("X-Webhook-Secret"))
    );

    let authorized_bad_json = client
        .post(server.url("/webhook"))
        .header("Authorization", "Bearer token-abc")
        .header("X-Webhook-Secret", "gateway-shared-secret")
        .json(&serde_json::json!({"persona_state": "missing message"}))
        .send()
        .await
        .expect("authorized request should complete");
    assert_eq!(authorized_bad_json.status(), StatusCode::BAD_REQUEST);
    let authorized_bad_json_body: Value = authorized_bad_json
        .json()
        .await
        .expect("authorized bad-json response should be json");
    assert!(
        authorized_bad_json_body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|msg| msg.contains("Invalid JSON"))
    );
}

struct CountingObserver {
    events: Arc<AtomicUsize>,
    metrics: Arc<AtomicUsize>,
    flushes: Arc<AtomicUsize>,
}

impl CountingObserver {
    fn new(events: Arc<AtomicUsize>, metrics: Arc<AtomicUsize>, flushes: Arc<AtomicUsize>) -> Self {
        Self {
            events,
            metrics,
            flushes,
        }
    }
}

impl Observer for CountingObserver {
    fn record_event(&self, _event: &ObserverEvent) {
        self.events.fetch_add(1, Ordering::SeqCst);
    }

    fn record_metric(&self, _metric: &ObserverMetric) {
        self.metrics.fetch_add(1, Ordering::SeqCst);
    }

    fn flush(&self) {
        self.flushes.fetch_add(1, Ordering::SeqCst);
    }

    fn name(&self) -> &str {
        "counting"
    }
}

#[test]
fn observability_deterministic_factory_and_fanout_path() {
    let cfg = ObservabilityConfig {
        backend: "prometheus".to_string(),
    };
    let observer = create_observer(&cfg);
    assert_eq!(observer.name(), "prometheus");

    let event_counter_a = Arc::new(AtomicUsize::new(0));
    let metric_counter_a = Arc::new(AtomicUsize::new(0));
    let flush_counter_a = Arc::new(AtomicUsize::new(0));
    let event_counter_b = Arc::new(AtomicUsize::new(0));
    let metric_counter_b = Arc::new(AtomicUsize::new(0));
    let flush_counter_b = Arc::new(AtomicUsize::new(0));

    let multi = MultiObserver::new(vec![
        Box::new(CountingObserver::new(
            Arc::clone(&event_counter_a),
            Arc::clone(&metric_counter_a),
            Arc::clone(&flush_counter_a),
        )),
        Box::new(CountingObserver::new(
            Arc::clone(&event_counter_b),
            Arc::clone(&metric_counter_b),
            Arc::clone(&flush_counter_b),
        )),
    ]);

    multi.record_event(&ObserverEvent::HeartbeatTick);
    multi.record_metric(&ObserverMetric::TokensUsed(42));
    multi.flush();

    assert_eq!(event_counter_a.load(Ordering::SeqCst), 1);
    assert_eq!(event_counter_b.load(Ordering::SeqCst), 1);
    assert_eq!(metric_counter_a.load(Ordering::SeqCst), 1);
    assert_eq!(metric_counter_b.load(Ordering::SeqCst), 1);
    assert_eq!(flush_counter_a.load(Ordering::SeqCst), 1);
    assert_eq!(flush_counter_b.load(Ordering::SeqCst), 1);
}
