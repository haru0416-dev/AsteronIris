use std::fmt;
use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow};

use asteroniris::memory::{
    CapabilitySupport, ForgetMode, ForgetStatus, Memory, MemoryCategory,
    capability_matrix_for_memory, ensure_forget_mode_supported,
};

use super::memory_harness;

const REPORT_PATH: &str = ".sisyphus/evidence/task-19-parity-report.csv";

#[derive(Debug, Clone)]
struct LifecycleBaseline {
    resolve_present: bool,
    recall_contains_slot: bool,
    hard_forget_applied: bool,
    hard_forget_complete: bool,
    hard_forget_status: ForgetStatus,
}

#[derive(Debug, Clone)]
struct ReportRow {
    backend: &'static str,
    scenario: &'static str,
    mode: &'static str,
    support: &'static str,
    verdict: &'static str,
    status: &'static str,
    applied: bool,
    complete: bool,
    degraded: bool,
    detail: String,
}

impl ReportRow {
    fn to_csv_line(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{}",
            self.backend,
            self.scenario,
            self.mode,
            self.support,
            self.verdict,
            self.status,
            self.applied,
            self.complete,
            self.degraded,
            csv_escape(&self.detail)
        )
    }
}

fn csv_escape(value: &str) -> String {
    let escaped = value.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn support_label(support: CapabilitySupport) -> &'static str {
    match support {
        CapabilitySupport::Supported => "SUPPORTED",
        CapabilitySupport::Degraded => "DEGRADED",
        CapabilitySupport::Unsupported => "UNSUPPORTED",
    }
}

fn mode_label(mode: ForgetMode) -> &'static str {
    match mode {
        ForgetMode::Soft => "soft",
        ForgetMode::Hard => "hard",
        ForgetMode::Tombstone => "tombstone",
    }
}

fn status_label(status: ForgetStatus) -> &'static str {
    match status {
        ForgetStatus::Complete => "complete",
        ForgetStatus::Incomplete => "incomplete",
        ForgetStatus::DegradedNonComplete => "degraded_non_complete",
        ForgetStatus::NotApplied => "not_applied",
    }
}

fn ensure_explicit_contract(
    backend: &'static str,
    mode: ForgetMode,
    support: CapabilitySupport,
    supported_preflight: bool,
    outcome: &asteroniris::memory::ForgetOutcome,
) -> Result<&'static str> {
    let mode_name = mode_label(mode);
    match support {
        CapabilitySupport::Supported => {
            if !supported_preflight {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=supported preflight_rejected"
                ));
            }
            if outcome.degraded || !outcome.applied || !outcome.complete {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=supported degraded={} applied={} complete={} status={}",
                    outcome.degraded,
                    outcome.applied,
                    outcome.complete,
                    status_label(outcome.status)
                ));
            }
            if outcome.status != ForgetStatus::Complete {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=supported status={} expected=complete",
                    status_label(outcome.status)
                ));
            }
            Ok("PASS")
        }
        CapabilitySupport::Degraded => {
            if !supported_preflight {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=degraded preflight_rejected"
                ));
            }
            if !outcome.degraded
                || outcome.complete
                || outcome.status != ForgetStatus::DegradedNonComplete
            {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=degraded degraded={} complete={} status={} expected=degraded_non_complete",
                    outcome.degraded,
                    outcome.complete,
                    status_label(outcome.status)
                ));
            }
            Ok("DEGRADED")
        }
        CapabilitySupport::Unsupported => {
            if supported_preflight {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=unsupported preflight_allowed"
                ));
            }
            if !outcome.degraded
                || outcome.complete
                || outcome.status != ForgetStatus::DegradedNonComplete
            {
                return Err(anyhow!(
                    "UNEXPECTED_DRIFT backend={backend} mode={mode_name} support=unsupported degraded={} complete={} status={} expected=degraded_non_complete",
                    outcome.degraded,
                    outcome.complete,
                    status_label(outcome.status)
                ));
            }
            Ok("UNSUPPORTED")
        }
    }
}

async fn collect_backend_report(
    backend: &'static str,
    memory: &dyn Memory,
    baseline: &LifecycleBaseline,
) -> Result<Vec<ReportRow>> {
    let mut rows = Vec::new();
    let matrix = capability_matrix_for_memory(memory);

    let entity = format!("task19-{backend}");
    let lifecycle_key = format!("{backend}.lifecycle");
    memory_harness::append_test_event(
        memory,
        &entity,
        &lifecycle_key,
        "backend parity lifecycle payload",
        MemoryCategory::Core,
    )
    .await;

    let resolved = memory_harness::resolve_slot_value(memory, &entity, &lifecycle_key).await;
    let resolve_present = resolved.as_deref() == Some("backend parity lifecycle payload");
    if resolve_present != baseline.resolve_present {
        return Err(anyhow!(
            "UNEXPECTED_DRIFT backend={backend} scenario=resolve_present got={resolve_present} expected={}",
            baseline.resolve_present
        ));
    }
    rows.push(ReportRow {
        backend,
        scenario: "store_resolve",
        mode: "n/a",
        support: "AUTHORITATIVE",
        verdict: "PASS",
        status: "n/a",
        applied: true,
        complete: true,
        degraded: false,
        detail: "resolve_slot matched authoritative sqlite behavior".to_string(),
    });

    let recalled =
        memory_harness::recall_scoped_items(memory, &entity, "lifecycle payload", 10).await;
    let recall_contains_slot = recalled
        .iter()
        .any(|item| item.value.contains("backend parity lifecycle payload"));
    if recall_contains_slot != baseline.recall_contains_slot {
        return Err(anyhow!(
            "UNEXPECTED_DRIFT backend={backend} scenario=recall_contains_slot got={recall_contains_slot} expected={}",
            baseline.recall_contains_slot
        ));
    }
    rows.push(ReportRow {
        backend,
        scenario: "recall_scoped",
        mode: "n/a",
        support: "AUTHORITATIVE",
        verdict: "PASS",
        status: "n/a",
        applied: true,
        complete: true,
        degraded: false,
        detail: "recall returned authoritative lifecycle key".to_string(),
    });

    for mode in [ForgetMode::Soft, ForgetMode::Hard, ForgetMode::Tombstone] {
        let slot_key = format!("{backend}.forget.{}", mode_label(mode));
        memory_harness::append_test_event(
            memory,
            &entity,
            &slot_key,
            "erase-me",
            MemoryCategory::Core,
        )
        .await;

        let support = matrix.support_for_forget_mode(mode);
        let preflight = ensure_forget_mode_supported(memory, mode).is_ok();
        let outcome = memory
            .forget_slot(&entity, &slot_key, mode, "task-19 parity contract")
            .await?;
        let verdict = ensure_explicit_contract(backend, mode, support, preflight, &outcome)?;

        if mode == ForgetMode::Hard
            && support == CapabilitySupport::Supported
            && (outcome.applied != baseline.hard_forget_applied
                || outcome.complete != baseline.hard_forget_complete
                || outcome.status != baseline.hard_forget_status)
        {
            return Err(anyhow!(
                "UNEXPECTED_DRIFT backend={backend} scenario=hard_forget_parity applied={} complete={} status={} expected_applied={} expected_complete={} expected_status={}",
                outcome.applied,
                outcome.complete,
                status_label(outcome.status),
                baseline.hard_forget_applied,
                baseline.hard_forget_complete,
                status_label(baseline.hard_forget_status)
            ));
        }

        rows.push(ReportRow {
            backend,
            scenario: "forget_mode",
            mode: mode_label(mode),
            support: support_label(support),
            verdict,
            status: status_label(outcome.status),
            applied: outcome.applied,
            complete: outcome.complete,
            degraded: outcome.degraded,
            detail: format!(
                "mode={} contract={} checks={}",
                mode_label(mode),
                support_label(support),
                outcome.artifact_checks.len()
            ),
        });
    }

    Ok(rows)
}

async fn sqlite_baseline(memory: &dyn Memory) -> Result<LifecycleBaseline> {
    let entity = "task19-baseline";
    let slot = "sqlite.baseline.lifecycle";
    memory_harness::append_test_event(
        memory,
        entity,
        slot,
        "baseline payload",
        MemoryCategory::Core,
    )
    .await;

    let resolve_present = memory_harness::resolve_slot_value(memory, entity, slot)
        .await
        .as_deref()
        == Some("baseline payload");
    let recall_contains_slot =
        memory_harness::recall_scoped_items(memory, entity, "baseline payload", 10)
            .await
            .iter()
            .any(|item| item.value.contains("baseline payload"));
    let hard = memory
        .forget_slot(entity, slot, ForgetMode::Hard, "task-19 baseline")
        .await?;

    Ok(LifecycleBaseline {
        resolve_present,
        recall_contains_slot,
        hard_forget_applied: hard.applied,
        hard_forget_complete: hard.complete,
        hard_forget_status: hard.status,
    })
}

fn write_report(rows: &[ReportRow], path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("report path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;

    let mut report = String::from(
        "backend,scenario,mode,support,verdict,status,applied,complete,degraded,detail\n",
    );
    for row in rows {
        report.push_str(&row.to_csv_line());
        report.push('\n');
    }

    fs::write(path, report)?;
    Ok(())
}

#[tokio::test]
async fn memory_backend_parity_matrix() {
    let (_tmp_sqlite, sqlite) = memory_harness::sqlite_fixture().await;
    let (_tmp_lancedb, lancedb) = memory_harness::lancedb_fixture();
    let (_tmp_markdown, markdown) = memory_harness::markdown_fixture();

    let baseline = sqlite_baseline(&sqlite)
        .await
        .expect("sqlite baseline should be established");

    let mut rows = Vec::new();
    rows.extend(
        collect_backend_report("sqlite", &sqlite, &baseline)
            .await
            .expect("sqlite parity scenarios should pass"),
    );
    rows.extend(
        collect_backend_report("lancedb", &lancedb, &baseline)
            .await
            .expect("lancedb parity scenarios should satisfy parity/degraded contract"),
    );
    rows.extend(
        collect_backend_report("markdown", &markdown, &baseline)
            .await
            .expect("markdown parity scenarios should satisfy parity/degraded contract"),
    );

    let report_path = Path::new(REPORT_PATH);
    write_report(&rows, report_path).expect("parity report should be persisted for CI diagnostics");

    let row_count = rows.len();
    println!(
        "task19 parity report rows={row_count} path={}",
        report_path.display()
    );
}

#[test]
fn memory_backend_parity_detects_drift() {
    let simulated = asteroniris::memory::ForgetOutcome {
        entity_id: "drift".to_string(),
        slot_key: "drift.slot".to_string(),
        mode: ForgetMode::Soft,
        applied: true,
        complete: true,
        degraded: false,
        status: ForgetStatus::Complete,
        artifact_checks: Vec::new(),
    };

    let drift = ensure_explicit_contract(
        "markdown",
        ForgetMode::Soft,
        CapabilitySupport::Degraded,
        true,
        &simulated,
    )
    .expect_err("drift detector must fail loudly for undocumented behavior");

    let msg = drift.to_string();
    assert!(
        msg.contains("UNEXPECTED_DRIFT backend=markdown mode=soft support=degraded"),
        "drift detector should emit deterministic marker, got: {msg}"
    );
}

impl fmt::Display for ReportRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_csv_line())
    }
}
