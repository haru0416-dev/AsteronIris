use crate::config::Config;
use crate::platform::cron::{CronJob, due_jobs, reschedule_after_run};
use crate::security::SecurityPolicy;
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{self, Duration};

mod agent_plan;
mod policy;
mod routes;

#[cfg(test)]
use crate::core::memory::{
    MemoryEventInput, MemoryEventType, MemorySource, PrivacyLevel, SourceKind, create_memory,
};

#[cfg(test)]
use agent_plan::{ensure_cron_jobs_schema, ensure_plan_execution_schema};
use agent_plan::{recover_interrupted_plan_jobs, run_agent_job_command};
use policy::enforce_policy_invariants;
use routes::{
    ParsedRoutedJob, parse_routed_job_command, run_ingestion_job_command, run_rss_poll_job_command,
    run_trend_aggregation_job_command, run_x_poll_job_command,
};
#[cfg(test)]
use routes::{
    RssPollItem, XRecentTweet, build_rss_poll_envelopes, build_x_poll_envelopes,
    parse_rss_items_from_xml, resolve_x_bearer_token, resolve_x_recent_search_endpoint,
};

const MIN_POLL_SECONDS: u64 = 5;
const ROUTE_MARKER_USER_SHELL: &str = "route=user-direct-shell";
const ROUTE_MARKER_AGENT_BLOCKED: &str = "route=agent-no-direct-shell";
const ROUTE_MARKER_AGENT_PLANNER: &str = "route=agent-planner";
const ROUTE_MARKER_INGEST_PIPELINE: &str = "route=user-ingestion-pipeline";
const ROUTE_MARKER_TREND_AGGREGATION: &str = "route=user-trend-aggregation";
const ROUTE_MARKER_X_POLL: &str = "route=user-x-poll";
const ROUTE_MARKER_RSS_POLL: &str = "route=user-rss-poll";
const TREND_AGGREGATION_LIMIT: usize = 20;
const TREND_AGGREGATION_TOP_ITEMS: usize = 5;
const INGEST_API_MIN_INTERVAL_SECONDS: i64 = 10;
const INGEST_RSS_MIN_INTERVAL_SECONDS: i64 = 30;
const X_RECENT_SEARCH_ENDPOINT: &str = "https://api.twitter.com/2/tweets/search/recent";

pub async fn run(config: Arc<Config>) -> Result<()> {
    let poll_secs = config.reliability.scheduler_poll_secs.max(MIN_POLL_SECONDS);
    let mut interval = time::interval(Duration::from_secs(poll_secs));
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    initialize_scheduler_state(&config);

    crate::runtime::diagnostics::health::mark_component_ok("scheduler");

    loop {
        interval.tick().await;
        crate::runtime::diagnostics::health::mark_component_ok("scheduler");

        let jobs = match due_jobs(&config, Utc::now()) {
            Ok(jobs) => jobs,
            Err(e) => {
                crate::runtime::diagnostics::health::mark_component_error(
                    "scheduler",
                    e.to_string(),
                );
                tracing::warn!("Scheduler query failed: {e}");
                continue;
            }
        };

        for job in jobs {
            crate::runtime::diagnostics::health::mark_component_ok("scheduler");
            let (success, output) = execute_job_with_retry(&config, &security, &job).await;

            if !success {
                crate::runtime::diagnostics::health::mark_component_error(
                    "scheduler",
                    format!("job {} failed", job.id),
                );
            }

            if let Err(e) = reschedule_after_run(&config, &job, success, &output) {
                crate::runtime::diagnostics::health::mark_component_error(
                    "scheduler",
                    e.to_string(),
                );
                tracing::warn!("Failed to persist scheduler run result: {e}");
            }
        }
    }
}

fn initialize_scheduler_state(config: &Config) {
    if let Err(error) = recover_interrupted_plan_jobs(config) {
        tracing::warn!(error = %error, "failed to recover interrupted plan executions");
    }
}

async fn execute_job_with_retry(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    let mut last_output = String::new();
    let retries = effective_retry_budget(config, job);
    let mut backoff_ms = config.reliability.provider_backoff_ms.max(200);

    for attempt in 0..=retries {
        let (success, output) = run_job_command(config, security, job).await;
        last_output = output;

        if success {
            return (true, last_output);
        }

        if last_output.starts_with("blocked by security policy:") {
            // Deterministic policy violations are not retryable.
            return (false, last_output);
        }

        if attempt < retries {
            let jitter_ms = u64::from(Utc::now().timestamp_subsec_millis() % 250);
            time::sleep(Duration::from_millis(backoff_ms + jitter_ms)).await;
            backoff_ms = (backoff_ms.saturating_mul(2)).min(30_000);
        }
    }

    (false, last_output)
}

fn effective_retry_budget(config: &Config, job: &CronJob) -> u32 {
    let retries = config.reliability.scheduler_retries;
    if job.origin == crate::platform::cron::CronJobOrigin::Agent {
        retries.min(job.max_attempts.saturating_sub(1))
    } else {
        retries
    }
}

async fn run_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    match job.origin {
        crate::platform::cron::CronJobOrigin::User => {
            run_user_job_command(config, security, job).await
        }
        crate::platform::cron::CronJobOrigin::Agent => {
            run_agent_job_command(config, security, job).await
        }
    }
}

pub async fn execute_job_once_for_integration(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    run_job_command(config, security, job).await
}

async fn run_user_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    if let Some(parsed) = parse_routed_job_command(&job.command) {
        return match parsed {
            ParsedRoutedJob::Ingestion(parsed) => {
                run_ingestion_job_command(config, security, parsed).await
            }
            ParsedRoutedJob::TrendAggregation(parsed) => {
                run_trend_aggregation_job_command(config, security, parsed).await
            }
            ParsedRoutedJob::XPoll(parsed) => {
                run_x_poll_job_command(config, security, parsed).await
            }
            ParsedRoutedJob::RssPoll(parsed) => {
                run_rss_poll_job_command(config, security, parsed).await
            }
        };
    }

    if let Err(output) = enforce_policy_invariants(security, &job.command, ROUTE_MARKER_USER_SHELL)
    {
        return (false, output);
    }

    let output = Command::new("sh")
        .arg("-lc")
        .arg(&job.command)
        .current_dir(&config.workspace_dir)
        .output()
        .await;

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!(
                "{ROUTE_MARKER_USER_SHELL}\nstatus={}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                stdout.trim(),
                stderr.trim()
            );
            (output.status.success(), combined)
        }
        Err(e) => (
            false,
            format!("{ROUTE_MARKER_USER_SHELL}\nspawn error: {e}"),
        ),
    }
}

#[cfg(test)]
mod tests;
