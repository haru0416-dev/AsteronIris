use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::max;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EvalScenarioSpec {
    pub id: &'static str,
    pub success_target_percent: u8,
    pub min_cost_cents: u32,
    pub max_cost_cents: u32,
    pub min_latency_ms: u32,
    pub max_latency_ms: u32,
    pub retry_cap: u32,
}

#[derive(Debug, Clone)]
pub struct EvalSuiteSpec {
    pub name: &'static str,
    pub scenarios: Vec<EvalScenarioSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalSuiteSummary {
    pub suite: String,
    pub case_count: u32,
    pub success_rate_bps: u32,
    pub avg_cost_cents: u32,
    pub avg_latency_ms: u32,
    pub avg_retries_milli: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalReport {
    pub seed: u64,
    pub suites: Vec<EvalSuiteSummary>,
    pub summary_fingerprint: u64,
}

const BASELINE_REPORT_REQUIRED_COLUMNS: [&str; 5] =
    ["suite", "success-rate", "cost", "latency", "retries"];
const BASELINE_REPORT_HEADER: &str = "suite,success-rate,cost,latency,retries";

fn expected_baseline_columns() -> Vec<String> {
    BASELINE_REPORT_REQUIRED_COLUMNS
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn parse_csv_columns(line: &str) -> Vec<String> {
    line.split(',')
        .map(|entry| entry.trim().to_ascii_lowercase())
        .collect()
}

impl EvalReport {
    pub fn required_csv_columns() -> &'static [&'static str; 5] {
        &BASELINE_REPORT_REQUIRED_COLUMNS
    }

    pub fn render_csv(&self) -> String {
        let mut csv = String::from(BASELINE_REPORT_HEADER);
        csv.push('\n');
        for suite in &self.suites {
            let _ = writeln!(
                csv,
                "{},{},{},{}ms,{}",
                suite.suite,
                format_rate(suite.success_rate_bps),
                format_currency_cents(suite.avg_cost_cents),
                suite.avg_latency_ms,
                format_retries(suite.avg_retries_milli)
            );
        }
        csv
    }

    pub fn render_text_summary(&self, warning: Option<&str>) -> String {
        let mut lines = vec![
            format!("seed={}", self.seed),
            format!("summary_fingerprint={}", self.summary_fingerprint),
            String::from("columns=success-rate,cost,latency,retries"),
        ];

        for suite in &self.suites {
            lines.push(format!(
                "suite={} success-rate={}bps cost={}c latency={}ms retries={}milli",
                suite.suite,
                suite.success_rate_bps,
                suite.avg_cost_cents,
                suite.avg_latency_ms,
                suite.avg_retries_milli
            ));
        }

        if let Some(message) = warning {
            lines.push(format!("warning={message}"));
        }

        lines.join("\n") + "\n"
    }
}

pub fn validate_baseline_report_columns(csv: &str) -> Result<()> {
    let header = csv
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing eval report csv header"))?;
    let columns = parse_csv_columns(header);
    let expected = expected_baseline_columns();

    if columns.is_empty() {
        bail!("missing eval report csv header");
    }

    let mut seen = HashSet::new();
    let mut duplicates = Vec::new();
    for column in &columns {
        if !seen.insert(column.clone()) {
            duplicates.push(column.clone());
        }
    }

    if !duplicates.is_empty() {
        bail!("duplicate columns: {}", duplicates.join(", "));
    }

    if columns == expected {
        return Ok(());
    }

    let mut missing: Vec<String> = Vec::new();
    for required in &expected {
        if !columns.iter().any(|entry| entry == required) {
            missing.push(required.clone());
        }
    }

    let mut unexpected: Vec<String> = Vec::new();
    for column in &columns {
        if !expected.iter().any(|expected| expected == column) {
            unexpected.push(column.clone());
        }
    }

    if !missing.is_empty() {
        bail!("missing required columns: {}", missing.join(", "));
    }

    if !unexpected.is_empty() {
        bail!("unexpected report columns: {}", unexpected.join(", "));
    }

    bail!(
        "unexpected column order: expected {} got {}",
        expected.join(", "),
        columns.join(", ")
    )
}

fn format_rate(success_rate_bps: u32) -> String {
    format!("{:.2}%", f64::from(success_rate_bps) / 100.0)
}

fn format_currency_cents(avg_cost_cents: u32) -> String {
    format!("${:.2}", f64::from(avg_cost_cents) / 100.0)
}

fn format_retries(avg_retries_milli: u32) -> String {
    format!("{:.3}", f64::from(avg_retries_milli) / 1_000.0)
}

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

#[derive(Debug, Clone)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_bounded(&mut self, upper_exclusive: u64) -> u64 {
        if upper_exclusive == 0 {
            return 0;
        }
        self.next_u64() % upper_exclusive
    }
}

fn bounded_inclusive(min: u32, max: u32, sample: u64) -> u32 {
    if min >= max {
        return min;
    }
    let span = u64::from(max - min) + 1;
    min + u32_saturating_from_u64(sample % span)
}

fn u32_saturating_from_u64(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn mix_seed(base_seed: u64, suite_name: &str, scenario_id: &str) -> u64 {
    let mut mixed = base_seed ^ fnv1a64(suite_name.as_bytes());
    mixed = mixed.rotate_left(17) ^ fnv1a64(scenario_id.as_bytes());
    mixed
}

fn fingerprint_summary(seed: u64, suites: &[EvalSuiteSummary]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325_u64 ^ seed;
    for suite in suites {
        hash = hash_combine(hash, suite.suite.as_bytes());
        hash ^= u64::from(suite.case_count);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
        hash ^= u64::from(suite.success_rate_bps);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
        hash ^= u64::from(suite.avg_cost_cents);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
        hash ^= u64::from(suite.avg_latency_ms);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
        hash ^= u64::from(suite.avg_retries_milli);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
    }
    hash
}

fn hash_combine(mut state: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        state ^= u64::from(byte);
        state = state.wrapping_mul(0x1000_0000_01B3);
    }
    state
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01B3);
    }
    hash
}
