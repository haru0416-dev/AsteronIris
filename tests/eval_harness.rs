use std::path::PathBuf;

use asteroniris::eval::{
    default_baseline_suites, detect_seed_change_warning, write_evidence_files, EvalHarness,
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

    assert!(csv.starts_with("suite,success-rate,cost,latency,retries"));
}
