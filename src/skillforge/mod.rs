//! `SkillForge` — Skill auto-discovery, evaluation, and integration engine.
//!
//! Pipeline: Scout → Evaluate → Integrate
//! Discovers skills from external sources, scores them, and generates
//! AsteronIris-compatible manifests for qualified candidates.

pub mod evaluate;
pub mod integrate;
pub mod scout;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use self::evaluate::{EvalResult, Evaluator, Recommendation};
use self::integrate::Integrator;
use self::scout::{ClawHubScout, GitHubScout, HuggingFaceScout, Scout, ScoutResult, ScoutSource};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct SkillForgeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_auto_integrate")]
    pub auto_integrate: bool,
    #[serde(default = "default_sources")]
    pub sources: Vec<String>,
    #[serde(default = "default_scan_interval")]
    pub scan_interval_hours: u64,
    #[serde(default = "default_min_score")]
    pub min_score: f64,
    /// Optional GitHub personal-access token for higher rate limits.
    #[serde(default)]
    pub github_token: Option<String>,
    #[serde(default)]
    pub clawhub_token: Option<String>,
    #[serde(default)]
    pub clawhub_base_url: Option<String>,
    /// Directory where integrated skills are written.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
}

fn default_auto_integrate() -> bool {
    true
}
fn default_sources() -> Vec<String> {
    vec!["github".into(), "clawhub".into()]
}
fn default_scan_interval() -> u64 {
    24
}
fn default_min_score() -> f64 {
    0.7
}
fn default_output_dir() -> String {
    "./skills".into()
}

impl Default for SkillForgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_integrate: default_auto_integrate(),
            sources: default_sources(),
            scan_interval_hours: default_scan_interval(),
            min_score: default_min_score(),
            github_token: None,
            clawhub_token: None,
            clawhub_base_url: None,
            output_dir: default_output_dir(),
        }
    }
}

impl std::fmt::Debug for SkillForgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillForgeConfig")
            .field("enabled", &self.enabled)
            .field("auto_integrate", &self.auto_integrate)
            .field("sources", &self.sources)
            .field("scan_interval_hours", &self.scan_interval_hours)
            .field("min_score", &self.min_score)
            .field("github_token", &self.github_token.as_ref().map(|_| "***"))
            .field("clawhub_token", &self.clawhub_token.as_ref().map(|_| "***"))
            .field("clawhub_base_url", &self.clawhub_base_url)
            .field("output_dir", &self.output_dir)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// ForgeReport — summary of a single pipeline run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeReport {
    pub discovered: usize,
    pub evaluated: usize,
    pub auto_integrated: usize,
    pub manual_review: usize,
    pub skipped: usize,
    pub results: Vec<EvalResult>,
}

// ---------------------------------------------------------------------------
// SkillForge
// ---------------------------------------------------------------------------

pub struct SkillForge {
    config: SkillForgeConfig,
    evaluator: Evaluator,
    integrator: Integrator,
}

impl SkillForge {
    pub fn new(config: SkillForgeConfig) -> Self {
        let evaluator = Evaluator::new(config.min_score);
        let integrator = Integrator::new(config.output_dir.clone());
        Self {
            config,
            evaluator,
            integrator,
        }
    }

    /// Run the full pipeline: Scout → Evaluate → Integrate.
    #[allow(clippy::too_many_lines)]
    pub async fn forge(&self) -> Result<ForgeReport> {
        if !self.config.enabled {
            warn!("SkillForge is disabled — skipping");
            return Ok(ForgeReport {
                discovered: 0,
                evaluated: 0,
                auto_integrated: 0,
                manual_review: 0,
                skipped: 0,
                results: vec![],
            });
        }

        // --- Scout ----------------------------------------------------------
        let mut candidates: Vec<ScoutResult> = Vec::new();

        for src in &self.config.sources {
            let source: ScoutSource = src.parse().unwrap(); // Infallible
            match source {
                ScoutSource::GitHub => {
                    let scout = GitHubScout::new(self.config.github_token.as_deref());
                    match scout.discover().await {
                        Ok(mut found) => {
                            info!(count = found.len(), "GitHub scout returned candidates");
                            candidates.append(&mut found);
                        }
                        Err(e) => {
                            warn!(error = %e, "GitHub scout failed, continuing with other sources");
                        }
                    }
                }
                ScoutSource::HuggingFace => {
                    let scout = HuggingFaceScout::new();
                    match scout.discover().await {
                        Ok(mut found) => {
                            info!(
                                count = found.len(),
                                "HuggingFace scout returned candidates"
                            );
                            candidates.append(&mut found);
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
                                "HuggingFace scout failed, continuing with other sources"
                            );
                        }
                    }
                }
                ScoutSource::ClawHub => {
                    let scout = ClawHubScout::new(
                        self.config.clawhub_base_url.as_deref(),
                        self.config.clawhub_token.as_deref(),
                    );
                    match scout.discover().await {
                        Ok(mut found) => {
                            info!(count = found.len(), "ClawHub scout returned candidates");
                            candidates.append(&mut found);
                        }
                        Err(e) => {
                            warn!(error = %e, "ClawHub scout failed, continuing with other sources");
                        }
                    }
                }
            }
        }

        // Deduplicate by URL
        scout::dedup(&mut candidates);
        let discovered = candidates.len();
        info!(discovered, "Total unique candidates after dedup");

        // --- Evaluate -------------------------------------------------------
        let results: Vec<EvalResult> = candidates
            .into_iter()
            .map(|c| self.evaluator.evaluate(c))
            .collect();
        let evaluated = results.len();

        // --- Integrate ------------------------------------------------------
        let mut auto_integrated = 0usize;
        let mut manual_review = 0usize;
        let mut skipped = 0usize;

        for res in &results {
            match res.recommendation {
                Recommendation::Auto => {
                    if self.config.auto_integrate {
                        match self.integrator.integrate(&res.candidate) {
                            Ok(_) => {
                                auto_integrated += 1;
                            }
                            Err(e) => {
                                warn!(
                                    skill = res.candidate.name.as_str(),
                                    error = %e,
                                    "Integration failed for candidate, continuing"
                                );
                            }
                        }
                    } else {
                        // Count as would-be auto but not actually integrated
                        manual_review += 1;
                    }
                }
                Recommendation::Manual => {
                    manual_review += 1;
                }
                Recommendation::Skip => {
                    skipped += 1;
                }
            }
        }

        info!(
            auto_integrated,
            manual_review, skipped, "Forge pipeline complete"
        );

        Ok(ForgeReport {
            discovered,
            evaluated,
            auto_integrated,
            manual_review,
            skipped,
            results,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    static HF_ENV_LOCK: Mutex<()> = Mutex::new(());

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
            .expect(2)
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
        let _guard = HF_ENV_LOCK.lock().unwrap();
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

        assert_eq!(report.discovered, 1, "huggingface should return one candidate");
        assert_eq!(report.evaluated, 1, "discovered candidate should be evaluated");
        assert_eq!(report.results[0].candidate.source, ScoutSource::HuggingFace);
        assert_eq!(report.results[0].candidate.owner, "openai");
        assert!(report.results[0].candidate.has_license);

        unsafe {
            std::env::remove_var("ASTERONIRIS_SKILLFORGE_HF_API_BASE");
        }
    }

    #[tokio::test]
    async fn skillforge_hf_rate_limit_handling() {
        let _guard = HF_ENV_LOCK.lock().unwrap();
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/models"))
            .and(query_param("search", "asteroniris skill"))
            .respond_with(ResponseTemplate::new(429))
            .expect(1)
            .mount(&server)
            .await;

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

        assert_eq!(report.discovered, 0, "rate-limited source should skip candidates");
        assert_eq!(report.evaluated, 0, "no candidates should be evaluated");

        unsafe {
            std::env::remove_var("ASTERONIRIS_SKILLFORGE_HF_API_BASE");
        }
    }
}
