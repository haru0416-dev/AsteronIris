use crate::config::Config;
use crate::cron::{CronJob, due_jobs, reschedule_after_run};
use crate::security::SecurityPolicy;
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::process::Command;
use tokio::time::{self, Duration};

const MIN_POLL_SECONDS: u64 = 5;
const ROUTE_MARKER_USER_SHELL: &str = "route=user-direct-shell";
const ROUTE_MARKER_AGENT_BLOCKED: &str = "route=agent-no-direct-shell";

pub async fn run(config: Arc<Config>) -> Result<()> {
    let poll_secs = config.reliability.scheduler_poll_secs.max(MIN_POLL_SECONDS);
    let mut interval = time::interval(Duration::from_secs(poll_secs));
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    crate::diagnostics::health::mark_component_ok("scheduler");

    loop {
        interval.tick().await;

        let jobs = match due_jobs(&config, Utc::now()) {
            Ok(jobs) => jobs,
            Err(e) => {
                crate::diagnostics::health::mark_component_error("scheduler", e.to_string());
                tracing::warn!("Scheduler query failed: {e}");
                continue;
            }
        };

        for job in jobs {
            crate::diagnostics::health::mark_component_ok("scheduler");
            let (success, output) = execute_job_with_retry(&config, &security, &job).await;

            if !success {
                crate::diagnostics::health::mark_component_error(
                    "scheduler",
                    format!("job {} failed", job.id),
                );
            }

            if let Err(e) = reschedule_after_run(&config, &job, success, &output) {
                crate::diagnostics::health::mark_component_error("scheduler", e.to_string());
                tracing::warn!("Failed to persist scheduler run result: {e}");
            }
        }
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
    if job.origin == crate::cron::CronJobOrigin::Agent {
        retries.min(job.max_attempts.saturating_sub(1))
    } else {
        retries
    }
}

fn is_env_assignment(word: &str) -> bool {
    word.contains('=')
        && word
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
}

fn strip_wrapping_quotes(token: &str) -> &str {
    token.trim_matches(|c| c == '"' || c == '\'')
}

fn forbidden_path_argument(security: &SecurityPolicy, command: &str) -> Option<String> {
    let mut normalized = command.to_string();
    for sep in ["&&", "||"] {
        normalized = normalized.replace(sep, "\x00");
    }
    for sep in ['\n', ';', '|'] {
        normalized = normalized.replace(sep, "\x00");
    }

    for segment in normalized.split('\x00') {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        // Skip leading env assignments and executable token.
        let mut idx = 0;
        while idx < tokens.len() && is_env_assignment(tokens[idx]) {
            idx += 1;
        }
        if idx >= tokens.len() {
            continue;
        }
        idx += 1;

        for token in &tokens[idx..] {
            let candidate = strip_wrapping_quotes(token);
            if candidate.is_empty() || candidate.starts_with('-') || candidate.contains("://") {
                continue;
            }

            let looks_like_path = candidate.starts_with('/')
                || candidate.starts_with("./")
                || candidate.starts_with("../")
                || candidate.starts_with("~/")
                || candidate.contains('/');

            if looks_like_path && !security.is_path_allowed(candidate) {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

fn policy_denial(route_marker: &str, reason: impl Into<String>) -> String {
    format!("{route_marker}\n{}", reason.into())
}

fn enforce_policy_invariants(
    security: &SecurityPolicy,
    command: &str,
    route_marker: &str,
) -> Result<(), String> {
    if !security.is_command_allowed(command) {
        return Err(policy_denial(
            route_marker,
            format!("blocked by security policy: command not allowed: {command}"),
        ));
    }

    if let Some(path) = forbidden_path_argument(security, command) {
        return Err(policy_denial(
            route_marker,
            format!("blocked by security policy: forbidden path argument: {path}"),
        ));
    }

    if let Err(policy_error) = security.consume_action_and_cost(0) {
        return Err(policy_denial(route_marker, policy_error));
    }

    Ok(())
}

async fn run_job_command(
    config: &Config,
    security: &SecurityPolicy,
    job: &CronJob,
) -> (bool, String) {
    match job.origin {
        crate::cron::CronJobOrigin::User => run_user_job_command(config, security, job).await,
        crate::cron::CronJobOrigin::Agent => run_agent_job_command(security, job),
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

fn run_agent_job_command(security: &SecurityPolicy, job: &CronJob) -> (bool, String) {
    if let Err(output) =
        enforce_policy_invariants(security, &job.command, ROUTE_MARKER_AGENT_BLOCKED)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::cron::{
        AGENT_PENDING_CAP, CronJobKind, CronJobMetadata, CronJobOrigin, add_job_with_metadata,
        due_jobs, list_jobs,
    };
    use crate::security::SecurityPolicy;
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

    fn test_job(command: &str) -> CronJob {
        CronJob {
            id: "test-job".into(),
            expression: "* * * * *".into(),
            command: command.into(),
            next_run: Utc::now(),
            last_run: None,
            last_status: None,
            job_kind: CronJobKind::User,
            origin: CronJobOrigin::User,
            expires_at: None,
            max_attempts: 1,
        }
    }

    fn agent_metadata(expires_at: Option<chrono::DateTime<Utc>>) -> CronJobMetadata {
        CronJobMetadata {
            job_kind: CronJobKind::Agent,
            origin: CronJobOrigin::Agent,
            expires_at,
            max_attempts: 3,
        }
    }

    #[tokio::test]
    async fn run_job_command_success() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let job = test_job("echo scheduler-ok");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("scheduler-ok"));
        assert!(output.contains("status=exit status: 0"));
    }

    #[tokio::test]
    async fn run_job_command_failure() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let job = test_job("ls definitely_missing_file_for_scheduler_test");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("definitely_missing_file_for_scheduler_test"));
        assert!(output.contains("status=exit status:"));
    }

    #[tokio::test]
    async fn run_job_command_blocks_disallowed_command() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["echo".into()];
        let job = test_job("curl https://evil.example");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("blocked by security policy"));
        assert!(output.contains("command not allowed"));
    }

    #[tokio::test]
    async fn run_job_command_blocks_forbidden_path_argument() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["cat".into()];
        let job = test_job("cat /etc/passwd");
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("blocked by security policy"));
        assert!(output.contains("forbidden path argument"));
        assert!(output.contains("/etc/passwd"));
    }

    #[tokio::test]
    async fn execute_job_with_retry_recovers_after_first_failure() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.reliability.scheduler_retries = 1;
        config.reliability.provider_backoff_ms = 1;
        config.autonomy.allowed_commands = vec!["sh".into()];
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        std::fs::write(
            config.workspace_dir.join("retry-once.sh"),
            "#!/bin/sh\nif [ -f retry-ok.flag ]; then\n  echo recovered\n  exit 0\nfi\ntouch retry-ok.flag\nexit 1\n",
        )
        .unwrap();
        let job = test_job("sh ./retry-once.sh");

        let (success, output) = execute_job_with_retry(&config, &security, &job).await;
        assert!(success);
        assert!(output.contains("recovered"));
    }

    #[tokio::test]
    async fn execute_job_with_retry_exhausts_attempts() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.reliability.scheduler_retries = 1;
        config.reliability.provider_backoff_ms = 1;
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let job = test_job("ls always_missing_for_retry_test");

        let (success, output) = execute_job_with_retry(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("always_missing_for_retry_test"));
    }

    #[tokio::test]
    async fn run_job_command_policy_blocks_when_action_limit_is_exhausted() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.max_actions_per_hour = 0;
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
        let job = test_job("echo should-not-run");

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("action limit"));
    }

    #[tokio::test]
    async fn scheduler_agent_jobs_never_direct_shell() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["sh".into()];
        let marker_file = "agent-shell-marker.txt";
        let marker_path = config.workspace_dir.join(marker_file);
        let command = format!("sh -c 'touch {marker_file}'");
        let mut job = test_job(&command);
        job.job_kind = CronJobKind::Agent;
        job.origin = CronJobOrigin::Agent;
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(!success);
        assert_eq!(
            output,
            "route=agent-no-direct-shell\nblocked by security policy: agent jobs cannot execute direct shell path"
        );
        assert!(!marker_path.exists());
    }

    #[tokio::test]
    async fn scheduler_user_jobs_still_execute_expected_path() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.autonomy.allowed_commands = vec!["sh".into()];
        let marker_file = "user-shell-marker.txt";
        let marker_path = config.workspace_dir.join(marker_file);
        let command = format!("sh -c 'touch {marker_file}'");
        let job = test_job(&command);
        let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

        let (success, output) = run_job_command(&config, &security, &job).await;
        assert!(success, "{output}");
        assert!(output.contains("route=user-direct-shell"));
        assert!(output.contains("status=exit status: 0"));
        assert!(marker_path.exists());
    }

    #[test]
    fn scheduler_agent_queue_bounded() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let expires_at = Some(Utc::now() + ChronoDuration::hours(1));

        for idx in 0..AGENT_PENDING_CAP {
            let command = format!("echo queue-{idx}");
            add_job_with_metadata(
                &config,
                "*/5 * * * *",
                &command,
                &agent_metadata(expires_at),
            )
            .unwrap();
        }

        let err = add_job_with_metadata(
            &config,
            "*/5 * * * *",
            "echo overflow",
            &agent_metadata(expires_at),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("agent-origin queue cap reached (5 pending jobs)")
        );
    }

    #[test]
    fn scheduler_expires_agent_jobs() {
        let tmp = TempDir::new().unwrap();
        let config = test_config(&tmp);
        let expired_at = Some(Utc::now() - ChronoDuration::minutes(1));

        let _job = add_job_with_metadata(
            &config,
            "*/5 * * * *",
            "echo expired",
            &agent_metadata(expired_at),
        )
        .unwrap();

        let jobs = due_jobs(&config, Utc::now()).unwrap();
        assert!(jobs.is_empty());

        let remaining = list_jobs(&config).unwrap();
        assert!(remaining.is_empty());
    }

    #[test]
    fn scheduler_agent_retry_budget_is_bounded_by_max_attempts() {
        let tmp = TempDir::new().unwrap();
        let mut config = test_config(&tmp);
        config.reliability.scheduler_retries = 9;

        let mut job = test_job("echo bounded-retries");
        job.origin = CronJobOrigin::Agent;
        job.max_attempts = 3;

        assert_eq!(effective_retry_budget(&config, &job), 2);

        job.max_attempts = 1;
        assert_eq!(effective_retry_budget(&config, &job), 0);
    }
}
