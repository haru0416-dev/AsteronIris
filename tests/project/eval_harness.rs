use std::path::PathBuf;

use asteroniris::eval::{
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
