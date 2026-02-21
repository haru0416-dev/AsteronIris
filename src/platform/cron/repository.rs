use super::expression::{next_run_for, parse_max_attempts, parse_rfc3339};
use super::types::{AGENT_PENDING_CAP, CronJob, CronJobKind, CronJobMetadata, CronJobOrigin};
use crate::config::Config;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use uuid::Uuid;

pub fn add_job(config: &Config, expression: &str, command: &str) -> Result<CronJob> {
    add_job_with_metadata(config, expression, command, &CronJobMetadata::default())
}

pub fn add_job_with_metadata(
    config: &Config,
    expression: &str,
    command: &str,
    metadata: &CronJobMetadata,
) -> Result<CronJob> {
    let now = Utc::now();
    let next_run = next_run_for(expression, now)?;
    let id = Uuid::new_v4().to_string();
    let max_attempts = metadata.max_attempts.max(1);

    with_connection(config, |conn| {
        if metadata.origin.is_agent() {
            cleanup_expired_jobs(conn, now)?;
            let pending = pending_agent_jobs(conn, now)?;
            if pending >= AGENT_PENDING_CAP {
                anyhow::bail!("agent-origin queue cap reached ({AGENT_PENDING_CAP} pending jobs)");
            }
        }

        conn.execute(
            "INSERT INTO cron_jobs (
                id, expression, command, created_at, next_run, job_kind, origin, expires_at, max_attempts
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                expression,
                command,
                now.to_rfc3339(),
                next_run.to_rfc3339(),
                metadata.job_kind.as_db(),
                metadata.origin.as_db(),
                metadata.expires_at.as_ref().map(DateTime::to_rfc3339),
                max_attempts
            ],
        )
        .context("Failed to insert cron job")?;
        Ok(())
    })?;

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

pub fn list_jobs(config: &Config) -> Result<Vec<CronJob>> {
    with_connection(config, |conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT id, expression, command, next_run, last_run, last_status,
                    job_kind, origin, expires_at, max_attempts
             FROM cron_jobs ORDER BY next_run ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            let next_run_raw: String = row.get(3)?;
            let last_run_raw: Option<String> = row.get(4)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                next_run_raw,
                last_run_raw,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?;

        let mut jobs = Vec::new();
        for row in rows {
            let (
                id,
                expression,
                command,
                next_run_raw,
                last_run_raw,
                last_status,
                job_kind_raw,
                origin_raw,
                expires_at_raw,
                max_attempts_raw,
            ) = row?;
            jobs.push(CronJob {
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
            });
        }
        Ok(jobs)
    })
}

pub fn remove_job(config: &Config, id: &str) -> Result<()> {
    let changed = with_connection(config, |conn| {
        conn.execute("DELETE FROM cron_jobs WHERE id = ?1", params![id])
            .context("Failed to delete cron job")
    })?;

    if changed == 0 {
        anyhow::bail!("Cron job '{id}' not found");
    }

    println!("✅ Removed cron job {id}");
    Ok(())
}

pub fn due_jobs(config: &Config, now: DateTime<Utc>) -> Result<Vec<CronJob>> {
    with_connection(config, |conn| {
        cleanup_expired_jobs(conn, now)?;

        let mut stmt = conn.prepare_cached(
            "SELECT id, expression, command, next_run, last_run, last_status,
                    job_kind, origin, expires_at, max_attempts
             FROM cron_jobs
             WHERE next_run <= ?1
               AND (expires_at IS NULL OR expires_at > ?1)
             ORDER BY next_run ASC",
        )?;

        let rows = stmt.query_map(params![now.to_rfc3339()], |row| {
            let next_run_raw: String = row.get(3)?;
            let last_run_raw: Option<String> = row.get(4)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                next_run_raw,
                last_run_raw,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
            ))
        })?;

        let mut jobs = Vec::new();
        for row in rows {
            let (
                id,
                expression,
                command,
                next_run_raw,
                last_run_raw,
                last_status,
                job_kind_raw,
                origin_raw,
                expires_at_raw,
                max_attempts_raw,
            ) = row?;
            jobs.push(CronJob {
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
            });
        }
        Ok(jobs)
    })
}

pub fn reschedule_after_run(
    config: &Config,
    job: &CronJob,
    success: bool,
    output: &str,
) -> Result<()> {
    let now = Utc::now();
    let next_run = next_run_for(&job.expression, now)?;
    let status = if success { "ok" } else { "error" };

    with_connection(config, |conn| {
        conn.execute(
            "UPDATE cron_jobs
             SET next_run = ?1, last_run = ?2, last_status = ?3, last_output = ?4
             WHERE id = ?5",
            params![
                next_run.to_rfc3339(),
                now.to_rfc3339(),
                status,
                output,
                job.id
            ],
        )
        .context("Failed to update cron job run state")?;
        Ok(())
    })
}

fn cleanup_expired_jobs(conn: &Connection, now: DateTime<Utc>) -> Result<()> {
    conn.execute(
        "DELETE FROM cron_jobs WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        params![now.to_rfc3339()],
    )
    .context("Failed to cleanup expired cron jobs")?;
    Ok(())
}

fn pending_agent_jobs(conn: &Connection, now: DateTime<Utc>) -> Result<usize> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*)
         FROM cron_jobs
         WHERE origin = 'agent'
           AND (expires_at IS NULL OR expires_at > ?1)",
        params![now.to_rfc3339()],
        |row| row.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

fn add_column_if_missing(conn: &Connection, sql: &str) -> Result<()> {
    match conn.execute(sql, []) {
        Ok(_) => Ok(()),
        Err(error) => {
            if error.to_string().contains("duplicate column name") {
                Ok(())
            } else {
                Err(error.into())
            }
        }
    }
}

fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cron directory: {}", parent.display()))?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open cron DB: {}", db_path.display()))?;

    conn.execute_batch(
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
        );
        CREATE INDEX IF NOT EXISTS idx_cron_jobs_next_run ON cron_jobs(next_run);",
    )
    .context("Failed to initialize cron schema")?;

    add_column_if_missing(
        &conn,
        "ALTER TABLE cron_jobs ADD COLUMN job_kind TEXT NOT NULL DEFAULT 'user'",
    )?;
    add_column_if_missing(
        &conn,
        "ALTER TABLE cron_jobs ADD COLUMN origin TEXT NOT NULL DEFAULT 'user'",
    )?;
    add_column_if_missing(&conn, "ALTER TABLE cron_jobs ADD COLUMN expires_at TEXT")?;
    add_column_if_missing(
        &conn,
        "ALTER TABLE cron_jobs ADD COLUMN max_attempts INTEGER NOT NULL DEFAULT 1",
    )?;

    f(&conn)
}
