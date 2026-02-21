use asteroniris::config::Config;
use asteroniris::platform::cron::{CronJob, CronJobKind, CronJobOrigin};
use asteroniris::security::SecurityPolicy;
use chrono::Utc;
use tempfile::TempDir;

fn build_job(command: &str, job_kind: CronJobKind, origin: CronJobOrigin) -> CronJob {
    CronJob {
        id: format!("job-{job_kind:?}-{origin:?}"),
        expression: "* * * * *".to_string(),
        command: command.to_string(),
        next_run: Utc::now(),
        last_run: None,
        last_status: None,
        job_kind,
        origin,
        expires_at: None,
        max_attempts: 1,
    }
}

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn scheduler_routes_user_vs_agent() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = Config::default();
    config.workspace_dir = workspace.clone();
    config.autonomy.allowed_commands = vec!["sh".to_string()];
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let user_marker_file = "route-user-origin-marker.txt";
    let user_job = build_job(
        &format!("sh -c 'touch {user_marker_file}'"),
        CronJobKind::Agent,
        CronJobOrigin::User,
    );
    let (user_success, user_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config, &security, &user_job,
        )
        .await;

    assert!(user_success, "{user_output}");
    assert!(user_output.contains("route=user-direct-shell"));
    assert!(user_output.contains("status=exit status: 0"));
    assert!(workspace.join(user_marker_file).exists());

    let agent_marker_file = "route-agent-origin-marker.txt";
    let agent_job = build_job(
        &format!("sh -c 'touch {agent_marker_file}'"),
        CronJobKind::User,
        CronJobOrigin::Agent,
    );
    let (agent_success, agent_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config, &security, &agent_job,
        )
        .await;

    assert!(!agent_success);
    assert_eq!(
        agent_output,
        "route=agent-no-direct-shell\nblocked by security policy: agent jobs cannot execute direct shell path"
    );
    assert!(!workspace.join(agent_marker_file).exists());
}

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn autonomy_policy_applies_all_routes() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = Config::default();
    config.workspace_dir = workspace;
    config.autonomy.allowed_commands = vec!["echo".to_string()];
    config.autonomy.max_actions_per_hour = 1;
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let user_first = build_job("echo first", CronJobKind::User, CronJobOrigin::User);
    let (user_first_success, user_first_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config,
            &security,
            &user_first,
        )
        .await;
    assert!(user_first_success, "{user_first_output}");
    assert!(user_first_output.contains("route=user-direct-shell"));

    let agent_second = build_job("echo second", CronJobKind::Agent, CronJobOrigin::Agent);
    let (agent_second_success, agent_second_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config,
            &security,
            &agent_second,
        )
        .await;

    assert!(!agent_second_success);
    assert!(agent_second_output.contains("route=agent-no-direct-shell"));
    assert!(agent_second_output.contains("blocked by security policy: action limit exceeded"));
}

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn autonomy_policy_blocks_bypass() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = Config::default();
    config.workspace_dir = workspace;
    config.autonomy.allowed_commands = vec!["cat".to_string()];
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);

    let agent_disallowed = build_job(
        "sh -c 'touch bypass.txt'",
        CronJobKind::Agent,
        CronJobOrigin::Agent,
    );
    let (agent_disallowed_success, agent_disallowed_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config,
            &security,
            &agent_disallowed,
        )
        .await;

    assert!(!agent_disallowed_success);
    assert!(agent_disallowed_output.contains("route=agent-no-direct-shell"));
    assert!(agent_disallowed_output.contains("blocked by security policy"));
    assert!(agent_disallowed_output.contains("command not allowed"));

    let user_forbidden_path = build_job("cat /etc/passwd", CronJobKind::User, CronJobOrigin::User);
    let (user_forbidden_path_success, user_forbidden_path_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config,
            &security,
            &user_forbidden_path,
        )
        .await;

    assert!(!user_forbidden_path_success);
    assert!(user_forbidden_path_output.contains("route=user-direct-shell"));
    assert!(user_forbidden_path_output.contains("forbidden path argument"));

    let agent_forbidden_path =
        build_job("cat /etc/passwd", CronJobKind::Agent, CronJobOrigin::Agent);
    let (agent_forbidden_path_success, agent_forbidden_path_output) =
        asteroniris::platform::cron::scheduler::execute_job_once_for_integration(
            &config,
            &security,
            &agent_forbidden_path,
        )
        .await;

    assert!(!agent_forbidden_path_success);
    assert!(agent_forbidden_path_output.contains("route=agent-no-direct-shell"));
    assert!(agent_forbidden_path_output.contains("forbidden path argument"));
}
