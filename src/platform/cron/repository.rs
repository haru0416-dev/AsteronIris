use super::expression::{next_run_for, parse_max_attempts, parse_rfc3339};
use super::types::{AGENT_PENDING_CAP, CronJob, CronJobKind, CronJobMetadata, CronJobOrigin};
use crate::config::Config;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

pub async fn add_job(config: &Config, expression: &str, command: &str) -> Result<CronJob> {
    add_job_with_metadata(config, expression, command, &CronJobMetadata::default()).await
}

pub async fn add_job_with_metadata(
    config: &Config,
    expression: &str,
    command: &str,
    metadata: &CronJobMetadata,
) -> Result<CronJob> {
    let now = Utc::now();
    let next_run = next_run_for(expression, now)?;
    let id = Uuid::new_v4().to_string();
    let max_attempts = metadata.max_attempts.max(1);

    let pool = open_pool(config).await?;

    if metadata.origin.is_agent() {
        cleanup_expired_jobs(&pool, now).await?;
        let pending = pending_agent_jobs(&pool, now).await?;
        if pending >= AGENT_PENDING_CAP {
            anyhow::bail!("agent-origin queue cap reached ({AGENT_PENDING_CAP} pending jobs)");
        }
    }

    sqlx::query(
        "INSERT INTO cron_jobs (
            id, expression, command, created_at, next_run, job_kind, origin, expires_at, max_attempts
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(expression)
    .bind(command)
    .bind(now.to_rfc3339())
    .bind(next_run.to_rfc3339())
    .bind(metadata.job_kind.as_db())
    .bind(metadata.origin.as_db())
    .bind(metadata.expires_at.as_ref().map(DateTime::to_rfc3339))
    .bind(i64::from(max_attempts))
    .execute(&pool)
    .await
    .context("Failed to insert cron job")?;

    Ok(CronJob {
        id,
        expression: expression.to_string(),
        command: command.to_string(),
        next_run,
        last_run: None,
        last_status: None,
        job_kind: metadata.job_kind,
        origin: metadata.origin,
        expires_at: metadata.expires_at,
        max_attempts,
    })
}

pub async fn list_jobs(config: &Config) -> Result<Vec<CronJob>> {
    let pool = open_pool(config).await?;
    let rows = sqlx::query(
        "SELECT id, expression, command, next_run, last_run, last_status,
                job_kind, origin, expires_at, max_attempts
         FROM cron_jobs ORDER BY next_run ASC",
    )
    .fetch_all(&pool)
    .await?;

    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        jobs.push(row_to_cron_job(&row)?);
    }
    Ok(jobs)
}

pub async fn remove_job(config: &Config, id: &str) -> Result<()> {
    let pool = open_pool(config).await?;
    let result = sqlx::query("DELETE FROM cron_jobs WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .context("Failed to delete cron job")?;

    if result.rows_affected() == 0 {
        anyhow::bail!("Cron job '{id}' not found");
    }

    println!("Removed cron job {id}");
    Ok(())
}

pub async fn due_jobs(config: &Config, now: DateTime<Utc>) -> Result<Vec<CronJob>> {
    let pool = open_pool(config).await?;
    cleanup_expired_jobs(&pool, now).await?;

    let rows = sqlx::query(
        "SELECT id, expression, command, next_run, last_run, last_status,
                job_kind, origin, expires_at, max_attempts
         FROM cron_jobs
         WHERE next_run <= ?
           AND (expires_at IS NULL OR expires_at > ?)
         ORDER BY next_run ASC",
    )
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .fetch_all(&pool)
    .await?;

    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        jobs.push(row_to_cron_job(&row)?);
    }
    Ok(jobs)
}

pub async fn reschedule_after_run(
    config: &Config,
    job: &CronJob,
    success: bool,
    output: &str,
) -> Result<()> {
    let now = Utc::now();
    let next_run = next_run_for(&job.expression, now)?;
    let status = if success { "ok" } else { "error" };

    let pool = open_pool(config).await?;
    sqlx::query(
        "UPDATE cron_jobs
         SET next_run = ?, last_run = ?, last_status = ?, last_output = ?
         WHERE id = ?",
    )
    .bind(next_run.to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(status)
    .bind(output)
    .bind(&job.id)
    .execute(&pool)
    .await
    .context("Failed to update cron job run state")?;

    Ok(())
}

// ── Pool management ─────────────────────────────────────────────────────────

pub(crate) async fn open_pool(config: &Config) -> Result<SqlitePool> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create cron directory: {}", parent.display()))?;
    }

    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .with_context(|| format!("Failed to open cron DB: {}", db_path.display()))?;

    ensure_schema(&pool).await?;
    Ok(pool)
}

async fn ensure_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cron_jobs (
            id          TEXT PRIMARY KEY,
            expression  TEXT NOT NULL,
            command     TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            next_run    TEXT NOT NULL,
            last_run    TEXT,
            last_status TEXT,
            last_output TEXT,
            job_kind    TEXT NOT NULL DEFAULT 'user',
            origin      TEXT NOT NULL DEFAULT 'user',
            expires_at  TEXT,
            max_attempts INTEGER NOT NULL DEFAULT 1
        )",
    )
    .execute(pool)
    .await
    .context("Failed to create cron_jobs table")?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_cron_jobs_next_run ON cron_jobs(next_run)")
        .execute(pool)
        .await
        .context("Failed to create cron_jobs index")?;

    Ok(())
}

// ── Internal helpers ────────────────────────────────────────────────────────

async fn cleanup_expired_jobs(pool: &SqlitePool, now: DateTime<Utc>) -> Result<()> {
    sqlx::query("DELETE FROM cron_jobs WHERE expires_at IS NOT NULL AND expires_at <= ?")
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .context("Failed to cleanup expired cron jobs")?;
    Ok(())
}

async fn pending_agent_jobs(pool: &SqlitePool, now: DateTime<Utc>) -> Result<usize> {
    let row = sqlx::query(
        "SELECT COUNT(*)
         FROM cron_jobs
         WHERE origin = 'agent'
           AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(now.to_rfc3339())
    .fetch_one(pool)
    .await?;

    let count: i64 = row.get(0);
    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

fn row_to_cron_job(row: &sqlx::sqlite::SqliteRow) -> Result<CronJob> {
    let id: String = row.get("id");
    let expression: String = row.get("expression");
    let command: String = row.get("command");
    let next_run_raw: String = row.get("next_run");
    let last_run_raw: Option<String> = row.get("last_run");
    let last_status: Option<String> = row.get("last_status");
    let job_kind_raw: String = row.get("job_kind");
    let origin_raw: String = row.get("origin");
    let expires_at_raw: Option<String> = row.get("expires_at");
    let max_attempts_raw: i64 = row.get("max_attempts");

    Ok(CronJob {
        id,
        expression,
        command,
        next_run: parse_rfc3339(&next_run_raw)?,
        last_run: match last_run_raw {
            Some(raw) => Some(parse_rfc3339(&raw)?),
            None => None,
        },
        last_status,
        job_kind: CronJobKind::from_db(&job_kind_raw),
        origin: CronJobOrigin::from_db(&origin_raw),
        expires_at: match expires_at_raw {
            Some(raw) => Some(parse_rfc3339(&raw)?),
            None => None,
        },
        max_attempts: parse_max_attempts(max_attempts_raw),
    })
}
