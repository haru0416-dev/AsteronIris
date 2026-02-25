use serde_json::Value;
use std::fs;
use std::path::Path;

const CONTRADICTION_RATIO_SLO_MAX: f64 = 0.20;

pub(super) fn run_memory_hygiene_tick(config: &crate::config::Config) {
    if !config.memory.hygiene_enabled {
        return;
    }

    if let Err(error) = crate::memory::hygiene::run_if_due(&config.memory, &config.workspace_dir) {
        tracing::warn!(%error, "memory hygiene tick failed");
    }

    if let Ok(Some(total)) = contradiction_mark_total(&config.workspace_dir) {
        tracing::info!(
            contradiction_mark_total = total,
            "memory contradiction metric snapshot"
        );
    }

    if let Ok(Some(total)) = belief_promotion_total(&config.workspace_dir) {
        tracing::info!(
            belief_promotion_total = total,
            "memory promotion metric snapshot"
        );
    }

    if let Ok(Some(total)) = stale_trend_purge_total(&config.workspace_dir) {
        tracing::info!(
            stale_trend_purge_total = total,
            "memory stale trend metric snapshot"
        );
    }

    evaluate_memory_slo(&config.workspace_dir);
}

pub(super) fn evaluate_memory_slo(workspace_dir: &Path) {
    let Ok(Some(ratio)) = contradiction_ratio(workspace_dir) else {
        return;
    };

    if ratio > CONTRADICTION_RATIO_SLO_MAX {
        tracing::warn!(
            contradiction_ratio = ratio,
            "contradiction_ratio_slo_violation ratio={ratio:.3} threshold={CONTRADICTION_RATIO_SLO_MAX:.3}"
        );
    }
}

/// Count contradiction-marked events using a synchronous sqlx query via a
/// blocking Tokio runtime. The heartbeat tick path runs on a dedicated thread,
/// so we use `block_in_place` to bridge into async sqlx.
pub(super) fn contradiction_mark_total(workspace_dir: &Path) -> anyhow::Result<Option<u64>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let url = format!("sqlite://{}?mode=ro", db_path.display());
    let rt = tokio::runtime::Handle::try_current();
    let count = match rt {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(async {
                let pool = sqlx::SqlitePool::connect(&url).await?;
                let row = sqlx::query("SELECT COUNT(*) FROM memory_events WHERE event_type = ?")
                    .bind("contradiction_marked")
                    .fetch_one(&pool)
                    .await?;
                let c: i64 = sqlx::Row::get(&row, 0);
                Ok::<_, anyhow::Error>(c)
            })
        })?,
        Err(_) => return Ok(None),
    };
    Ok(Some(u64::try_from(count).unwrap_or(0)))
}

pub(super) fn contradiction_ratio(workspace_dir: &Path) -> anyhow::Result<Option<f64>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let url = format!("sqlite://{}?mode=ro", db_path.display());
    let rt = tokio::runtime::Handle::try_current();
    let (total, contradicted) = match rt {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(async {
                let pool = sqlx::SqlitePool::connect(&url).await?;

                let row = sqlx::query("SELECT COUNT(*) FROM retrieval_units")
                    .fetch_one(&pool)
                    .await?;
                let total: i64 = sqlx::Row::get(&row, 0);

                let row = sqlx::query(
                    "SELECT COUNT(*) FROM retrieval_units WHERE contradiction_penalty > ?",
                )
                .bind(0.0_f64)
                .fetch_one(&pool)
                .await?;
                let contradicted: i64 = sqlx::Row::get(&row, 0);

                Ok::<_, anyhow::Error>((total, contradicted))
            })
        })?,
        Err(_) => return Ok(None),
    };

    if total <= 0 {
        return Ok(Some(0.0));
    }

    let total_u32 = u32::try_from(total).unwrap_or(u32::MAX);
    let contradicted_u32 = u32::try_from(contradicted).unwrap_or(u32::MAX);

    Ok(Some(
        f64::from(contradicted_u32) / f64::from(total_u32.max(1)),
    ))
}

pub(super) fn belief_promotion_total(workspace_dir: &Path) -> anyhow::Result<Option<u64>> {
    let db_path = workspace_dir.join("memory").join("brain.db");
    if !db_path.exists() {
        return Ok(None);
    }

    let url = format!("sqlite://{}?mode=ro", db_path.display());
    let rt = tokio::runtime::Handle::try_current();
    let count = match rt {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(async {
                    let pool = sqlx::SqlitePool::connect(&url).await?;
                    let row = sqlx::query(
                        "SELECT COUNT(*) FROM retrieval_units WHERE promotion_status IN ('candidate', 'promoted')",
                    )
                    .fetch_one(&pool)
                    .await?;
                    let c: i64 = sqlx::Row::get(&row, 0);
                    Ok::<_, anyhow::Error>(c)
                })
        })?,
        Err(_) => return Ok(None),
    };
    Ok(Some(u64::try_from(count).unwrap_or(0)))
}

pub(super) fn stale_trend_purge_total(workspace_dir: &Path) -> anyhow::Result<Option<u64>> {
    let state_path = workspace_dir
        .join("state")
        .join("memory_hygiene_state.json");
    if !state_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(state_path)?;
    let json: Value = serde_json::from_str(&raw)?;
    let total = json
        .get("last_report")
        .and_then(|v| v.get("lifecycle"))
        .and_then(|v| v.get("stale_trend_demoted"))
        .and_then(Value::as_u64);
    Ok(total)
}
