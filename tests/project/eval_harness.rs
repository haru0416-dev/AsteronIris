use std::path::PathBuf;

use asteroniris::core::eval::{
    EvalHarness, default_baseline_suites, detect_seed_change_warning,
    validate_baseline_report_columns, write_evidence_files,
};

#[test]
fn eval_harness_reproducible_seed() {
    let suites = default_baseline_suites();
    let harness = EvalHarness::new(0xA57E_0013);

    let first = harness.run(&suites);
    let second = harness.run(&suites);

    assert_eq!(first, second);
    assert!(detect_seed_change_warning(&first, &second).is_none());

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = write_evidence_files(&repo_root, &first, "eval-repro", None).unwrap();

    let csv = std::fs::read_to_string(&paths[1]).unwrap();
    assert!(csv.contains("success-rate"));
    assert!(csv.contains("cost"));
    assert!(csv.contains("latency"));
    assert!(csv.contains("retries"));
}

#[test]
fn eval_harness_baseline_report_csv_is_deterministic() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0013).run(&suites);

    let first_csv = report.render_csv();
    let second_csv = report.render_csv();

    assert_eq!(first_csv, second_csv);
}

#[test]
fn eval_harness_seed_change_diff_detected() {
    let suites = default_baseline_suites();
    let first = EvalHarness::new(0xA57E_0013).run(&suites);
    let second = EvalHarness::new(0xA57E_0014).run(&suites);

    assert_ne!(first.summary_fingerprint, second.summary_fingerprint);

    let warning = detect_seed_change_warning(&first, &second)
        .expect("seed change should emit warning when fingerprint changes");
    assert!(warning.contains("seed changed"));
    assert!(warning.contains("summary fingerprint changed"));

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let paths = write_evidence_files(&repo_root, &second, "eval-diff", Some(&warning)).unwrap();

    let summary = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(summary.contains("warning=seed changed"));
}

#[test]
fn eval_harness_baseline_report_has_required_columns() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0020).run(&suites);
    let csv = report.render_csv();

    let first_line = csv.lines().next().expect("report CSV must include header");
    assert_eq!(first_line, "suite,success-rate,cost,latency,retries");
    assert!(validate_baseline_report_columns(&csv).is_ok());
}

#[test]
fn eval_gate_requires_columns() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0021).run(&suites);
    let rendered_csv = report.render_csv();
    let header_and_rows: Vec<_> = rendered_csv.lines().collect();
    assert!(!header_and_rows.is_empty());

    let malformed_csv = format!(
        "suite,success-rate,cost\n{}",
        header_and_rows[1..].join("\n")
    );

    let err =
        validate_baseline_report_columns(&malformed_csv).expect_err("missing columns must fail");
    let message = err.to_string();
    assert!(message.contains("missing required columns"));
    assert!(message.contains("latency"));
    assert!(message.contains("retries"));
}

#[test]
fn eval_gate_rejects_permuted_columns() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0022).run(&suites);
    let rendered_csv = report.render_csv();
    let header_and_rows: Vec<_> = rendered_csv.lines().collect();
    assert!(!header_and_rows.is_empty());

    let malformed_csv = format!(
        "suite,retries,latency,cost,success-rate\n{}",
        header_and_rows[1..].join("\n")
    );

    let err =
        validate_baseline_report_columns(&malformed_csv).expect_err("permuted columns must fail");
    let message = err.to_string();
    assert!(message.contains("unexpected column order"));
    assert!(message.contains("suite"));
    assert!(message.contains("latency"));
    assert!(message.contains("retries"));
}

#[test]
fn eval_gate_rejects_extra_columns() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0023).run(&suites);
    let rendered_csv = report.render_csv();
    let header_and_rows: Vec<_> = rendered_csv.lines().collect();
    assert!(!header_and_rows.is_empty());

    let malformed_csv = format!(
        "suite,success-rate,cost,latency,retries,notes\n{}",
        header_and_rows[1..].join("\n")
    );

    let err =
        validate_baseline_report_columns(&malformed_csv).expect_err("extra columns must fail");
    let message = err.to_string();
    assert!(message.contains("unexpected report columns"));
    assert!(message.contains("notes"));
}

#[test]
fn eval_harness_includes_planner_memory_ingestion_suite() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0024).run(&suites);

    let suite = report
        .suites
        .iter()
        .find(|suite| suite.suite == "planner-memory-ingestion")
        .expect("planner-memory-ingestion suite should exist in baseline report");
    assert_eq!(suite.case_count, 3);
}

#[test]
fn eval_harness_summaries_are_sorted_by_suite_name() {
    let suites = default_baseline_suites();
    let report = EvalHarness::new(0xA57E_0025).run(&suites);

    let mut sorted = report
        .suites
        .iter()
        .map(|suite| suite.suite.clone())
        .collect::<Vec<_>>();
    let observed = sorted.clone();
    sorted.sort();

    assert_eq!(observed, sorted);
}

#[test]
fn eval_harness_baseline_contains_all_required_suite_names() {
    let suites = default_baseline_suites();
    let planner_suite = suites
        .iter()
        .find(|suite| suite.name == "planner-memory-ingestion")
        .expect("planner-memory-ingestion suite should exist");

    let ids = planner_suite
        .scenarios
        .iter()
        .map(|scenario| scenario.id.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    assert!(ids.contains("planner-success-rate"));
    assert!(ids.contains("memory-recall-precision"));
    assert!(ids.contains("ingestion-throughput"));

    let report = EvalHarness::new(0xA57E_0026).run(&suites);
    assert!(
        report
            .suites
            .iter()
            .any(|suite| suite.suite == "planner-memory-ingestion")
    );
}

#[test]
fn default_baseline_suite_inventory_is_stable() {
    let suites = default_baseline_suites();
    let names = suites
        .iter()
        .map(|suite| suite.name)
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(suites.len(), 3);
    assert!(names.contains("autonomy-regression"));
    assert!(names.contains("injection-defense-regression"));
    assert!(names.contains("planner-memory-ingestion"));
}

#[test]
fn eval_harness_is_deterministic_even_when_suite_input_order_changes() {
    let suites = default_baseline_suites();
    let mut reversed = suites.clone();
    reversed.reverse();

    let first = EvalHarness::new(0xA57E_0030).run(&suites);
    let second = EvalHarness::new(0xA57E_0030).run(&reversed);

    assert_eq!(first, second);
    assert_eq!(first.summary_fingerprint, second.summary_fingerprint);
}

#[test]
fn detect_seed_change_warning_reports_unchanged_fingerprint_branch() {
    let suites = default_baseline_suites();
    let previous = EvalHarness::new(0xA57E_0031).run(&suites);
    let mut current = previous.clone();
    current.seed = 0xA57E_0032;

    let warning = detect_seed_change_warning(&previous, &current)
        .expect("seed change should still return a warning message");
    assert!(warning.contains("seed changed"));
    assert!(warning.contains("summary fingerprint unchanged"));
}
