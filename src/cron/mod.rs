use crate::config::Config;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use cron::Schedule;
use rusqlite::{Connection, params};
use std::str::FromStr;
use uuid::Uuid;

pub mod scheduler;

#[derive(Debug, Clone)]
pub struct CronJob {
    pub id: String,
    pub expression: String,
    pub command: String,
    pub next_run: DateTime<Utc>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
    pub job_kind: CronJobKind,
    pub origin: CronJobOrigin,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronJobKind {
    User,
    Agent,
}

impl CronJobKind {
    fn as_db(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    fn from_db(value: &str) -> Self {
        if value.eq_ignore_ascii_case("agent") {
            Self::Agent
        } else {
            Self::User
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronJobOrigin {
    User,
    Agent,
}

impl CronJobOrigin {
    fn as_db(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    fn from_db(value: &str) -> Self {
        if value.eq_ignore_ascii_case("agent") {
            Self::Agent
        } else {
            Self::User
        }
    }

    fn is_agent(self) -> bool {
        self == Self::Agent
    }
}

#[derive(Debug, Clone)]
pub struct CronJobMetadata {
    pub job_kind: CronJobKind,
    pub origin: CronJobOrigin,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_attempts: u32,
}

impl Default for CronJobMetadata {
    fn default() -> Self {
        Self {
            job_kind: CronJobKind::User,
            origin: CronJobOrigin::User,
            expires_at: None,
            max_attempts: 1,
        }
    }
}

pub const AGENT_PENDING_CAP: usize = 5;

#[allow(clippy::needless_pass_by_value)]
pub fn handle_command(command: crate::CronCommands, config: &Config) -> Result<()> {
    match command {
        crate::CronCommands::List => {
            let jobs = list_jobs(config)?;
            if jobs.is_empty() {
                println!("No scheduled tasks yet.");
                println!("\nUsage:");
                println!("  asteroniris cron add '0 9 * * *' 'agent -m \"Good morning!\"'");
                return Ok(());
            }

            println!("🕒 Scheduled jobs ({}):", jobs.len());
            for job in jobs {
                let last_run = job
                    .last_run
                    .map_or_else(|| "never".into(), |d| d.to_rfc3339());
                let last_status = job.last_status.unwrap_or_else(|| "n/a".into());
                println!(
                    "- {} | {} | next={} | last={} ({})\n    cmd: {}",
                    job.id,
                    job.expression,
                    job.next_run.to_rfc3339(),
                    last_run,
                    last_status,
                    job.command
                );
            }
            Ok(())
        }
        crate::CronCommands::Add {
            expression,
            command,
        } => {
            let job = add_job(config, &expression, &command)?;
            println!("✅ Added cron job {}", job.id);
            println!("  Expr: {}", job.expression);
            println!("  Next: {}", job.next_run.to_rfc3339());
            println!("  Cmd : {}", job.command);
            Ok(())
        }
        crate::CronCommands::Remove { id } => remove_job(config, &id),
    }
}

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
        let mut stmt = conn.prepare(
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

        let mut stmt = conn.prepare(
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

fn next_run_for(expression: &str, from: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let normalized = normalize_expression(expression)?;
    let schedule = Schedule::from_str(&normalized)
        .with_context(|| format!("Invalid cron expression: {expression}"))?;
    schedule
        .after(&from)
        .next()
        .ok_or_else(|| anyhow::anyhow!("No future occurrence for expression: {expression}"))
}

fn normalize_expression(expression: &str) -> Result<String> {
    let expression = expression.trim();
    let field_count = expression.split_whitespace().count();

    match field_count {
        // standard crontab syntax: minute hour day month weekday
        5 => Ok(format!("0 {expression}")),
        // crate-native syntax includes seconds (+ optional year)
        6 | 7 => Ok(expression.to_string()),
        _ => anyhow::bail!(
            "Invalid cron expression: {expression} (expected 5, 6, or 7 fields, got {field_count})"
        ),
    }
}

fn parse_rfc3339(raw: &str) -> Result<DateTime<Utc>> {
    let parsed = DateTime::parse_from_rfc3339(raw)
        .with_context(|| format!("Invalid RFC3339 timestamp in cron DB: {raw}"))?;
    Ok(parsed.with_timezone(&Utc))
}

fn parse_max_attempts(raw: i64) -> u32 {
    u32::try_from(raw)
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(1)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use chrono::Duration as ChronoDuration;
    use tempfile::TempDir;

    fn test_config(tmp: &TempDir) -> Config {
        let config = Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Config::default()
        };
        std::fs::create_dir_all(&config.workspace_dir).unwrap();
        config
    }

    #[test]
    fn add_job_accepts_five_field_expression() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let job = add_job(&config, "*/5 * * * *", "echo ok").unwrap();

        assert_eq!(job.expression, "*/5 * * * *");
        assert_eq!(job.command, "echo ok");
        assert_eq!(job.job_kind, CronJobKind::User);
        assert_eq!(job.origin, CronJobOrigin::User);
        assert_eq!(job.expires_at, None);
        assert_eq!(job.max_attempts, 1);
    }

    #[test]
    fn add_job_rejects_invalid_field_count() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let err = add_job(&config, "* * * *", "echo bad").unwrap_err();
        assert!(err.to_string().contains("expected 5, 6, or 7 fields"));
    }

    #[test]
    fn add_list_remove_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let job = add_job(&config, "*/10 * * * *", "echo roundtrip").unwrap();
        let listed = list_jobs(&config).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, job.id);

        remove_job(&config, &job.id).unwrap();
        assert!(list_jobs(&config).unwrap().is_empty());
    }

    #[test]
    fn due_jobs_filters_by_timestamp() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let _job = add_job(&config, "* * * * *", "echo due").unwrap();

        let due_now = due_jobs(&config, Utc::now()).unwrap();
        assert!(due_now.is_empty(), "new job should not be due immediately");

        let far_future = Utc::now() + ChronoDuration::days(365);
        let due_future = due_jobs(&config, far_future).unwrap();
        assert_eq!(due_future.len(), 1, "job should be due in far future");
    }

    #[test]
    fn reschedule_after_run_persists_last_status_and_last_run() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);

        let job = add_job(&config, "*/15 * * * *", "echo run").unwrap();
        reschedule_after_run(&config, &job, false, "failed output").unwrap();

        let listed = list_jobs(&config).unwrap();
        let stored = listed.iter().find(|j| j.id == job.id).unwrap();
        assert_eq!(stored.last_status.as_deref(), Some("error"));
        assert!(stored.last_run.is_some());
    }
}
