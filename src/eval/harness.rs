use super::rng::{
    DeterministicRng, bounded_inclusive, fingerprint_summary, mix_seed, u32_saturating_from_u64,
};
use super::types::{EvalReport, EvalScenarioSpec, EvalSuiteSpec, EvalSuiteSummary};
use anyhow::Result;
use std::cmp::max;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EvalHarness {
    seed: u64,
}

impl EvalHarness {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub fn run(&self, suites: &[EvalSuiteSpec]) -> EvalReport {
        let mut ordered_suites = suites.to_vec();
        ordered_suites.sort_by(|a, b| a.name.cmp(b.name));

        let mut summaries = Vec::with_capacity(ordered_suites.len());
        for suite in &ordered_suites {
            let mut scenarios = suite.scenarios.clone();
            scenarios.sort_by(|a, b| a.id.cmp(b.id));

            let mut success_count = 0_u32;
            let mut total_cost = 0_u64;
            let mut total_latency = 0_u64;
            let mut total_retries = 0_u64;

            for scenario in &scenarios {
                let local_seed = mix_seed(self.seed, suite.name, scenario.id);
                let mut rng = DeterministicRng::new(local_seed);
                let roll = rng.next_bounded(100);
                let success = roll < u64::from(scenario.success_target_percent);
                if success {
                    success_count += 1;
                }

                let cost = bounded_inclusive(
                    scenario.min_cost_cents,
                    scenario.max_cost_cents,
                    rng.next_u64(),
                );
                let latency = bounded_inclusive(
                    scenario.min_latency_ms,
                    scenario.max_latency_ms,
                    rng.next_u64(),
                );

                let retries_sample = rng.next_bounded(u64::from(scenario.retry_cap) + 1);
                let retries = if success {
                    u32_saturating_from_u64(retries_sample)
                } else {
                    max(1, u32_saturating_from_u64(retries_sample))
                };

                total_cost += u64::from(cost);
                total_latency += u64::from(latency);
                total_retries += u64::from(retries);
            }

            let case_count = u32::try_from(scenarios.len()).unwrap_or(u32::MAX);
            let summary = EvalSuiteSummary {
                suite: suite.name.to_string(),
                case_count,
                success_rate_bps: (success_count * 10_000) / case_count,
                avg_cost_cents: u32_saturating_from_u64(total_cost / u64::from(case_count)),
                avg_latency_ms: u32_saturating_from_u64(total_latency / u64::from(case_count)),
                avg_retries_milli: u32_saturating_from_u64(
                    (total_retries * 1_000) / u64::from(case_count),
                ),
            };
            summaries.push(summary);
        }

        let summary_fingerprint = fingerprint_summary(self.seed, &summaries);
        EvalReport {
            seed: self.seed,
            suites: summaries,
            summary_fingerprint,
        }
    }
}

pub fn default_baseline_suites() -> Vec<EvalSuiteSpec> {
    vec![
        EvalSuiteSpec {
            name: "autonomy-regression",
            scenarios: vec![
                EvalScenarioSpec {
                    id: "bounded-repair-success",
                    success_target_percent: 93,
                    min_cost_cents: 8,
                    max_cost_cents: 23,
                    min_latency_ms: 80,
                    max_latency_ms: 190,
                    retry_cap: 2,
                },
                EvalScenarioSpec {
                    id: "policy-limit-enforced",
                    success_target_percent: 88,
                    min_cost_cents: 6,
                    max_cost_cents: 19,
                    min_latency_ms: 70,
                    max_latency_ms: 170,
                    retry_cap: 2,
                },
                EvalScenarioSpec {
                    id: "scheduler-agent-split",
                    success_target_percent: 90,
                    min_cost_cents: 7,
                    max_cost_cents: 21,
                    min_latency_ms: 90,
                    max_latency_ms: 210,
                    retry_cap: 3,
                },
                EvalScenarioSpec {
                    id: "temperature-clamp",
                    success_target_percent: 91,
                    min_cost_cents: 5,
                    max_cost_cents: 17,
                    min_latency_ms: 65,
                    max_latency_ms: 155,
                    retry_cap: 2,
                },
            ],
        },
        EvalSuiteSpec {
            name: "injection-defense-regression",
            scenarios: vec![
                EvalScenarioSpec {
                    id: "raw-payload-replay-blocked",
                    success_target_percent: 95,
                    min_cost_cents: 3,
                    max_cost_cents: 12,
                    min_latency_ms: 45,
                    max_latency_ms: 125,
                    retry_cap: 1,
                },
                EvalScenarioSpec {
                    id: "prompt-injection-writeback-denied",
                    success_target_percent: 92,
                    min_cost_cents: 4,
                    max_cost_cents: 15,
                    min_latency_ms: 50,
                    max_latency_ms: 130,
                    retry_cap: 1,
                },
                EvalScenarioSpec {
                    id: "sanitization-allows-low-risk",
                    success_target_percent: 94,
                    min_cost_cents: 4,
                    max_cost_cents: 13,
                    min_latency_ms: 40,
                    max_latency_ms: 120,
                    retry_cap: 1,
                },
                EvalScenarioSpec {
                    id: "marker-collision-detection",
                    success_target_percent: 90,
                    min_cost_cents: 5,
                    max_cost_cents: 14,
                    min_latency_ms: 55,
                    max_latency_ms: 140,
                    retry_cap: 2,
                },
            ],
        },
    ]
}

pub fn detect_seed_change_warning(previous: &EvalReport, current: &EvalReport) -> Option<String> {
    if previous.seed == current.seed {
        return None;
    }

    if previous.summary_fingerprint != current.summary_fingerprint {
        return Some(format!(
            "seed changed ({} -> {}) and summary fingerprint changed ({} -> {})",
            previous.seed, current.seed, previous.summary_fingerprint, current.summary_fingerprint
        ));
    }

    Some(format!(
        "seed changed ({} -> {}), summary fingerprint unchanged",
        previous.seed, current.seed
    ))
}

pub fn write_evidence_files(
    repo_root: &Path,
    report: &EvalReport,
    slug: &str,
    warning: Option<&str>,
) -> Result<Vec<PathBuf>> {
    let evidence_dir = repo_root.join(".sisyphus/evidence");
    fs::create_dir_all(&evidence_dir)?;

    let txt_path = evidence_dir.join(format!("task-13-{slug}.txt"));
    let csv_path = evidence_dir.join(format!("task-13-{slug}-baseline-report.csv"));
    let json_path = evidence_dir.join(format!("task-13-{slug}-baseline-report.json"));

    fs::write(&txt_path, report.render_text_summary(warning))?;
    fs::write(&csv_path, report.render_csv())?;
    fs::write(&json_path, serde_json::to_string_pretty(report)?)?;

    Ok(vec![txt_path, csv_path, json_path])
}
