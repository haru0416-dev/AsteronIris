use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write as _;

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
