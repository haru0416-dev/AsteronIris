use crate::config::MemoryConfig;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::filesystem::{
    archive_daily_memory_files, archive_session_files, purge_memory_archives,
    purge_session_archives,
};
use super::prune::{prune_conversation_rows, prune_v2_lifecycle_rows};

pub(super) const HYGIENE_INTERVAL_HOURS: i64 = 12;
const STATE_FILE: &str = "memory_hygiene_state.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct HygieneReport {
    archived_memory_files: u64,
    archived_session_files: u64,
    purged_memory_archives: u64,
    purged_session_archives: u64,
    pruned_conversation_rows: u64,
}

impl HygieneReport {
    fn total_actions(&self) -> u64 {
        self.archived_memory_files
            + self.archived_session_files
            + self.purged_memory_archives
            + self.purged_session_archives
            + self.pruned_conversation_rows
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct HygieneState {
    last_run_at: Option<String>,
    last_report: HygieneReport,
}

/// Run memory/session hygiene if the cadence window has elapsed.
///
/// This function is intentionally best-effort: callers should log and continue on failure.
pub fn run_if_due(config: &MemoryConfig, workspace_dir: &Path) -> Result<()> {
    if !config.hygiene_enabled {
        return Ok(());
    }

    if !should_run_now(workspace_dir)? {
        return Ok(());
    }

    let report = HygieneReport {
        archived_memory_files: archive_daily_memory_files(workspace_dir, config.archive_after_days)
            .context("archive daily memory files")?,
        archived_session_files: archive_session_files(workspace_dir, config.archive_after_days)
            .context("archive session files")?,
        purged_memory_archives: purge_memory_archives(workspace_dir, config.purge_after_days)
            .context("purge expired memory archives")?,
        purged_session_archives: purge_session_archives(workspace_dir, config.purge_after_days)
            .context("purge expired session archives")?,
        pruned_conversation_rows: prune_conversation_rows(
            workspace_dir,
            config.conversation_retention_days,
        )
        .context("prune stale conversation rows")?,
    };

    let _ = prune_v2_lifecycle_rows(workspace_dir, config)?;

    write_state(workspace_dir, &report).context("save hygiene state")?;

    if report.total_actions() > 0 {
        tracing::info!(
            "memory hygiene complete: archived_memory={} archived_sessions={} purged_memory={} purged_sessions={} pruned_conversation_rows={}",
            report.archived_memory_files,
            report.archived_session_files,
            report.purged_memory_archives,
            report.purged_session_archives,
            report.pruned_conversation_rows,
        );
    }

    Ok(())
}

fn should_run_now(workspace_dir: &Path) -> Result<bool> {
    let path = state_path(workspace_dir);
    if !path.exists() {
        return Ok(true);
    }

    let raw = fs::read_to_string(&path).context("read hygiene state file")?;
    let state: HygieneState = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(_) => return Ok(true),
    };

    let Some(last_run_at) = state.last_run_at else {
        return Ok(true);
    };

    let last = match DateTime::parse_from_rfc3339(&last_run_at) {
        Ok(ts) => ts.with_timezone(&Utc),
        Err(_) => return Ok(true),
    };

    Ok(Utc::now().signed_duration_since(last) >= Duration::hours(HYGIENE_INTERVAL_HOURS))
}

fn write_state(workspace_dir: &Path, report: &HygieneReport) -> Result<()> {
    let path = state_path(workspace_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create hygiene state directory")?;
    }

    let state = HygieneState {
        last_run_at: Some(Utc::now().to_rfc3339()),
        last_report: report.clone(),
    };
    let json = serde_json::to_vec_pretty(&state).context("serialize hygiene state")?;
    fs::write(path, json).context("write hygiene state to disk")?;
    Ok(())
}

fn state_path(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("state").join(STATE_FILE)
}
