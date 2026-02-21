use super::scout::ScoutSource;
use super::*;
use tokio::sync::Mutex;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

static HF_ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[tokio::test]
async fn disabled_forge_returns_empty_report() {
    let cfg = SkillForgeConfig {
        enabled: false,
        ..Default::default()
    };
    let forge = SkillForge::new(cfg);
    let report = forge.forge().await.unwrap();
    assert_eq!(report.discovered, 0);
    assert_eq!(report.auto_integrated, 0);
}

#[test]
fn default_config_values() {
    let cfg = SkillForgeConfig::default();
    assert!(!cfg.enabled);
    assert!(cfg.auto_integrate);
    assert_eq!(cfg.scan_interval_hours, 24);
    assert!((cfg.min_score - 0.7).abs() < f64::EPSILON);
    assert_eq!(cfg.sources, vec!["github", "clawhub"]);
    assert!(cfg.clawhub_token.is_none());
    assert!(cfg.clawhub_base_url.is_none());
}

#[tokio::test]
async fn skillforge_clawhub_discover() {
    let server = MockServer::start().await;
    let response = serde_json::json!({
        "results": [
            {
                "name": "clawhub-skill",
                "url": "https://github.com/clawhub-org/clawhub-skill",
                "description": "Skill from ClawHub",
                "stars": 88,
                "language": "Rust",
                "updated_at": "2026-02-01T12:00:00Z",
                "owner": { "login": "clawhub-org" },
                "has_license": true
            },
            {
                "name": "clawhub-skill-duplicate",
                "url": "https://github.com/clawhub-org/clawhub-skill"
            }
        ]
    });

    Mock::given(method("GET"))
        .and(path("/v1/skills"))
        .and(query_param("q", "asteroniris skill"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/v1/skills"))
        .and(query_param("q", "ai agent skill"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    let cfg = SkillForgeConfig {
        enabled: true,
        auto_integrate: false,
        sources: vec!["clawhub".to_string()],
        clawhub_base_url: Some(server.uri()),
        ..Default::default()
    };
    let forge = SkillForge::new(cfg);
    let report = forge.forge().await.unwrap();

    assert_eq!(report.discovered, 1);
    assert_eq!(report.evaluated, 1);
    assert_eq!(report.results[0].candidate.source, ScoutSource::ClawHub);
    assert_eq!(report.results[0].candidate.owner, "clawhub-org");
    assert!(report.results[0].candidate.has_license);
}

#[tokio::test]
async fn skillforge_clawhub_handles_auth_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/skills"))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("content-type", "application/json")
                .set_body_json(serde_json::json!({"message": "invalid token"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let cfg = SkillForgeConfig {
        enabled: true,
        auto_integrate: false,
        sources: vec!["clawhub".to_string()],
        clawhub_base_url: Some(server.uri()),
        clawhub_token: Some("bad-token".to_string()),
        ..Default::default()
    };
    let forge = SkillForge::new(cfg);
    let report = forge.forge().await.unwrap();

    assert_eq!(report.discovered, 0);
    assert_eq!(report.evaluated, 0);
    assert_eq!(report.auto_integrated, 0);
}

#[tokio::test]
async fn skillforge_hf_discover() {
    let _guard = HF_ENV_LOCK.lock().await;
    let server = MockServer::start().await;
    let response = serde_json::json!([
        {
            "id": "openai/agent-skill",
            "cardData": {
                "description": "Useful automation skill",
                "license": "apache-2.0"
            },
            "likes": 120,
            "tags": ["rust", "license:apache-2.0"],
            "lastModified": "2026-01-15T10:00:00Z"
        }
    ]);

    Mock::given(method("GET"))
        .and(path("/api/models"))
        .and(query_param("search", "asteroniris skill"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .expect(1)
        .mount(&server)
        .await;

    // SAFETY: Test-only env-var mutation. HF_ENV_LOCK (acquired above)
    // serialises all HuggingFace tests, preventing concurrent access.
    unsafe {
        std::env::set_var("ASTERONIRIS_SKILLFORGE_HF_API_BASE", server.uri());
    }

    let cfg = SkillForgeConfig {
        enabled: true,
        auto_integrate: false,
        sources: vec!["huggingface".to_string()],
        ..Default::default()
    };
    let forge = SkillForge::new(cfg);
    let report = forge.forge().await.unwrap();

    assert_eq!(
        report.discovered, 1,
        "huggingface should return one candidate"
    );
    assert_eq!(
        report.evaluated, 1,
        "discovered candidate should be evaluated"
    );
    assert_eq!(report.results[0].candidate.source, ScoutSource::HuggingFace);
    assert_eq!(report.results[0].candidate.owner, "openai");
    assert!(report.results[0].candidate.has_license);

    // SAFETY: Test-only cleanup; HF_ENV_LOCK is still held.
    unsafe {
        std::env::remove_var("ASTERONIRIS_SKILLFORGE_HF_API_BASE");
    }
}

#[tokio::test]
async fn skillforge_hf_rate_limit_handling() {
    let _guard = HF_ENV_LOCK.lock().await;
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/models"))
        .and(query_param("search", "asteroniris skill"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&server)
        .await;

    // SAFETY: Test-only env-var mutation. HF_ENV_LOCK (acquired above)
    // serialises all HuggingFace tests, preventing concurrent access.
    unsafe {
        std::env::set_var("ASTERONIRIS_SKILLFORGE_HF_API_BASE", server.uri());
    }

    let cfg = SkillForgeConfig {
        enabled: true,
        auto_integrate: false,
        sources: vec!["hf".to_string()],
        ..Default::default()
    };
    let forge = SkillForge::new(cfg);
    let report = forge.forge().await.unwrap();

    assert_eq!(
        report.discovered, 0,
        "rate-limited source should skip candidates"
    );
    assert_eq!(report.evaluated, 0, "no candidates should be evaluated");

    // SAFETY: Test-only cleanup; HF_ENV_LOCK is still held.
    unsafe {
        std::env::remove_var("ASTERONIRIS_SKILLFORGE_HF_API_BASE");
    }
}
