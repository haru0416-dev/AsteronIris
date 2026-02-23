use super::{ROUTE_MARKER_AGENT_BLOCKED, ROUTE_MARKER_AGENT_PLANNER};
use crate::config::Config;
use crate::core::planner::{PlanExecutor, PlanParser, ToolStepRunner};
use crate::core::tools::{ToolRegistry, default_middleware_chain, default_tools};
use crate::platform::cron::CronJob;
use crate::security::SecurityPolicy;
use chrono::Utc;
use rusqlite::{Connection, params};
use std::sync::Arc;
use uuid::Uuid;

#[allow(clippy::too_many_lines)]
pub(super) async fn run_agent_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    if let Some(raw_plan) = job.command.strip_prefix("plan:") {
        if let Err(policy_error) = security.consume_action_and_cost(0) {
            return (
                false,
                format!("{ROUTE_MARKER_AGENT_PLANNER}\nblocked by security policy: {policy_error}"),
            );
        }

        let mut plan = match PlanParser::parse(raw_plan.trim()) {
            Ok(plan) => plan,
            Err(error) => {
                let _ = persist_plan_execution(config, job, "parse_failed", 1, 0, 1, 0, raw_plan);
                return (
                    false,
                    format!("{ROUTE_MARKER_AGENT_PLANNER}\nplan parse failed: {error}"),
                );
            }
        };

        let security_arc = Arc::new(security.clone());
        let mut registry = ToolRegistry::new(default_middleware_chain());
        for tool in default_tools(&security_arc) {
            registry.register(tool);
        }

        let runner = ToolStepRunner::new(
            Arc::new(registry),
            crate::core::tools::middleware::ExecutionContext::from_security(security_arc),
        );
        let execution_id = begin_plan_execution(config, job, &plan.id, raw_plan).ok();
        let max_attempts = job.max_attempts.max(1);
        let mut attempts = 1_u32;
        let mut final_report = match PlanExecutor::execute(&mut plan, &runner).await {
            Ok(report) => report,
            Err(error) => {
                if let Some(execution_id) = execution_id.as_deref() {
                    let _ = finalize_plan_execution(
                        config,
                        execution_id,
                        "execution_error",
                        attempts,
                        0,
                        1,
                        0,
                    );
                } else {
                    let _ = persist_plan_execution(
                        config,
                        job,
                        "execution_error",
                        attempts,
                        0,
                        1,
                        0,
                        raw_plan,
                    );
                }
                return (
                    false,
                    format!("{ROUTE_MARKER_AGENT_PLANNER}\nplan execution failed: {error}"),
                );
            }
        };

        while !final_report.success && attempts < max_attempts {
            attempts = attempts.saturating_add(1);
            let Ok(mut retry_plan) = PlanParser::parse(raw_plan.trim()) else {
                break;
            };
            let Ok(retry_report) = PlanExecutor::execute(&mut retry_plan, &runner).await else {
                break;
            };
            final_report = retry_report;
        }

        let success = final_report.success;
        let retry_limit_reached = !success && attempts >= max_attempts;
        let output = format!(
            "{ROUTE_MARKER_AGENT_PLANNER}\nsuccess={}\nattempts={attempts}\nmax_attempts={max_attempts}\nretry_limit_reached={retry_limit_reached}\ncompleted={}\nfailed={}\nskipped={}",
            final_report.success,
            final_report.completed_steps.len(),
            final_report.failed_steps.len(),
            final_report.skipped_steps.len()
        );
        let status = if final_report.success {
            "completed"
        } else {
            "failed"
        };
        if let Some(execution_id) = execution_id.as_deref() {
            let _ = finalize_plan_execution(
                config,
                execution_id,
                status,
                attempts,
                final_report.completed_steps.len(),
                final_report.failed_steps.len(),
                final_report.skipped_steps.len(),
            );
        } else {
            let _ = persist_plan_execution(
                config,
                job,
                status,
                attempts,
                final_report.completed_steps.len(),
                final_report.failed_steps.len(),
                final_report.skipped_steps.len(),
                raw_plan,
            );
        }
        return (success, output);
    }

    if let Err(output) =
        super::policy::enforce_policy_invariants(security, &job.command, ROUTE_MARKER_AGENT_BLOCKED)
    {
        return (false, output);
    }

    (
        false,
        format!(
            "{ROUTE_MARKER_AGENT_BLOCKED}\nblocked by security policy: agent jobs cannot execute direct shell path"
        ),
    )
}

fn begin_plan_execution(
    config: &Config,
    job: &CronJob,
    plan_id: &str,
    plan_json: &str,
) -> anyhow::Result<String> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;
    let execution_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO plan_executions (
            id, job_id, plan_id, status, attempts,
            completed_steps, failed_steps, skipped_steps,
            plan_json, created_at
        ) VALUES (?1, ?2, ?3, 'running', 0, 0, 0, 0, ?4, ?5)",
        params![execution_id, job.id, plan_id, plan_json, now],
    )?;
    Ok(execution_id)
}

fn finalize_plan_execution(
    config: &Config,
    execution_id: &str,
    status: &str,
    attempts: u32,
    completed_steps: usize,
    failed_steps: usize,
    skipped_steps: usize,
) -> anyhow::Result<()> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;
    conn.execute(
        "UPDATE plan_executions
         SET status = ?2,
             attempts = ?3,
             completed_steps = ?4,
             failed_steps = ?5,
             skipped_steps = ?6
         WHERE id = ?1",
        params![
            execution_id,
            status,
            i64::from(attempts),
            i64::try_from(completed_steps).unwrap_or(0),
            i64::try_from(failed_steps).unwrap_or(0),
            i64::try_from(skipped_steps).unwrap_or(0)
        ],
    )?;
    Ok(())
}

pub(super) fn ensure_plan_execution_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS plan_executions (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            plan_id TEXT,
            status TEXT NOT NULL,
            attempts INTEGER NOT NULL,
            completed_steps INTEGER NOT NULL,
            failed_steps INTEGER NOT NULL,
            skipped_steps INTEGER NOT NULL,
            plan_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_plan_executions_job ON plan_executions(job_id);
        CREATE INDEX IF NOT EXISTS idx_plan_executions_created_at ON plan_executions(created_at);",
    )?;
    Ok(())
}

pub(super) fn ensure_cron_jobs_schema(conn: &Connection) -> anyhow::Result<()> {
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
    )?;
    Ok(())
}

pub(super) fn recover_interrupted_plan_jobs(config: &Config) -> anyhow::Result<usize> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if !db_path.exists() {
        return Ok(0);
    }

    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;
    ensure_cron_jobs_schema(&conn)?;

    let mut stmt = conn.prepare(
        "SELECT id, job_id, plan_json FROM plan_executions WHERE status = 'running' ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let now = Utc::now().to_rfc3339();
    let mut recovered = 0_usize;
    for row in rows {
        let (execution_id, job_id, plan_json) = row?;
        let changed = conn.execute(
            "UPDATE cron_jobs
             SET next_run = ?1,
                 last_status = 'recover_pending',
                 last_output = 'recovered_from_plan_execution',
                 max_attempts = CASE WHEN max_attempts < 1 THEN 3 ELSE max_attempts END
             WHERE id = ?2 AND origin = 'agent'",
            params![now, job_id],
        )?;

        if changed == 0 {
            conn.execute(
                "INSERT INTO cron_jobs (
                    id, expression, command, created_at, next_run,
                    last_run, last_status, last_output,
                    job_kind, origin, expires_at, max_attempts
                ) VALUES (?1, '*/5 * * * *', ?2, ?3, ?4, NULL, 'recover_pending', 'recovered_from_plan_execution', 'agent', 'agent', NULL, 3)",
                params![Uuid::new_v4().to_string(), format!("plan:{plan_json}"), now, now],
            )?;
        }

        conn.execute(
            "UPDATE plan_executions SET status = 'requeued', attempts = CASE WHEN attempts < 1 THEN 1 ELSE attempts END WHERE id = ?1",
            params![execution_id],
        )?;
        recovered = recovered.saturating_add(1);
    }

    Ok(recovered)
}

#[allow(clippy::too_many_arguments)]
fn persist_plan_execution(
    config: &Config,
    job: &CronJob,
    status: &str,
    attempts: u32,
    completed_steps: usize,
    failed_steps: usize,
    skipped_steps: usize,
    plan_json: &str,
) -> anyhow::Result<()> {
    let db_path = config.workspace_dir.join("cron").join("jobs.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    ensure_plan_execution_schema(&conn)?;

    let now = Utc::now().to_rfc3339();
    let execution_id = Uuid::new_v4().to_string();
    let plan_id = if let Ok(parsed) = PlanParser::parse(plan_json.trim()) {
        parsed.id
    } else {
        "unknown".to_string()
    };

    conn.execute(
        "INSERT INTO plan_executions (
            id, job_id, plan_id, status, attempts,
            completed_steps, failed_steps, skipped_steps,
            plan_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            execution_id,
            job.id,
            plan_id,
            status,
            i64::from(attempts),
            i64::try_from(completed_steps).unwrap_or(0),
            i64::try_from(failed_steps).unwrap_or(0),
            i64::try_from(skipped_steps).unwrap_or(0),
            plan_json,
            now
        ],
    )?;

    Ok(())
}
