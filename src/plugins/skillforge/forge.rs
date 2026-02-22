//! `SkillForge` — Core forge engine and pipeline report.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::config::SkillForgeConfig;
use super::evaluate::{EvalResult, Evaluator, Recommendation};
use super::integrate::Integrator;
use super::scout::{ClawHubScout, GitHubScout, HuggingFaceScout, Scout, ScoutResult, ScoutSource};

// ── ForgeReport — summary of a single pipeline run ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeReport {
    pub discovered: usize,
    pub evaluated: usize,
    pub auto_integrated: usize,
    pub manual_review: usize,
    pub skipped: usize,
    pub gate_rejected: usize,
    pub results: Vec<EvalResult>,
}

// ── SkillForge ───────────────────────────────────────────────────────────────

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
                gate_rejected: 0,
                results: vec![],
            });
        }

        // ── Scout ────────────────────────────────────────────────────────────
        let mut candidates: Vec<ScoutResult> = Vec::new();

        for src in &self.config.sources {
            let source: ScoutSource = src.parse().unwrap_or(ScoutSource::GitHub); // ScoutSource::from_str is infallible
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
                            info!(count = found.len(), "HuggingFace scout returned candidates");
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
        super::scout::dedup(&mut candidates);
        let discovered = candidates.len();
        info!(discovered, "Total unique candidates after dedup");

        // ── Evaluate ─────────────────────────────────────────────────────────
        let results: Vec<EvalResult> = candidates
            .into_iter()
            .map(|c| self.evaluator.evaluate(c))
            .collect();
        let evaluated = results.len();

        // ── Integrate ────────────────────────────────────────────────────────
        let mut auto_integrated = 0usize;
        let mut manual_review = 0usize;
        let mut skipped = 0usize;
        let mut gate_rejected = 0usize;

        for res in &results {
            if res.gate_verdict.is_rejected() {
                gate_rejected += 1;
                warn!(
                    skill = res.candidate.name.as_str(),
                    reasons = ?res.gate_verdict.reason_codes(),
                    "Skill rejected by security gate"
                );
                continue;
            }

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
            manual_review, skipped, gate_rejected, "Forge pipeline complete"
        );

        Ok(ForgeReport {
            discovered,
            evaluated,
            auto_integrated,
            manual_review,
            skipped,
            gate_rejected,
            results,
        })
    }
}
