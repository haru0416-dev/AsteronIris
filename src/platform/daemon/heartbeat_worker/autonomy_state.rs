use crate::runtime::observability::traits::AutonomyLifecycleSignal;
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn autonomy_level_to_str(level: crate::security::AutonomyLevel) -> &'static str {
    match level {
        crate::security::AutonomyLevel::ReadOnly => "read_only",
        crate::security::AutonomyLevel::Supervised => "supervised",
        crate::security::AutonomyLevel::Full => "full",
    }
}

fn read_last_autonomy_level(workspace_dir: &Path) -> Option<String> {
    let path = workspace_dir.join("state").join("autonomy_mode_state.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&raw)
        .ok()?
        .get("last")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn write_last_autonomy_level(workspace_dir: &Path, level: &str) -> Result<()> {
    let state_dir = workspace_dir.join("state");
    fs::create_dir_all(&state_dir)?;
    let path = state_dir.join("autonomy_mode_state.json");
    let payload = serde_json::json!({"last": level});
    fs::write(path, serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

pub(super) fn record_autonomy_mode_transition(
    config: &crate::config::Config,
    observer: &Arc<dyn crate::runtime::observability::Observer>,
) {
    let current = autonomy_level_to_str(config.autonomy.effective_autonomy_level()).to_string();
    if let Some(previous) = read_last_autonomy_level(&config.workspace_dir)
        && previous != current
    {
        observer.record_autonomy_lifecycle(AutonomyLifecycleSignal::ModeTransition);
        tracing::info!(from = previous, to = current, "autonomy mode transitioned");
    }
    let _ = write_last_autonomy_level(&config.workspace_dir, &current);
}
