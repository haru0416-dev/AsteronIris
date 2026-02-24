use super::*;
use crate::config::Config;
use chrono::{Duration as ChronoDuration, Utc};
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

#[tokio::test]
async fn add_job_accepts_five_field_expression() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let job = add_job(&config, "*/5 * * * *", "echo ok").await.unwrap();

    assert_eq!(job.expression, "*/5 * * * *");
    assert_eq!(job.command, "echo ok");
    assert_eq!(job.job_kind, CronJobKind::User);
    assert_eq!(job.origin, CronJobOrigin::User);
    assert_eq!(job.expires_at, None);
    assert_eq!(job.max_attempts, 1);
}

#[tokio::test]
async fn add_job_rejects_invalid_field_count() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let err = add_job(&config, "* * * *", "echo bad").await.unwrap_err();
    assert!(err.to_string().contains("expected 5, 6, or 7 fields"));
}

#[tokio::test]
async fn add_list_remove_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let job = add_job(&config, "*/10 * * * *", "echo roundtrip")
        .await
        .unwrap();
    let listed = list_jobs(&config).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, job.id);

    remove_job(&config, &job.id).await.unwrap();
    assert!(list_jobs(&config).await.unwrap().is_empty());
}

#[tokio::test]
async fn due_jobs_filters_by_timestamp() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let _job = add_job(&config, "* * * * *", "echo due").await.unwrap();

    let due_now = due_jobs(&config, Utc::now()).await.unwrap();
    assert!(due_now.is_empty(), "new job should not be due immediately");

    let far_future = Utc::now() + ChronoDuration::days(365);
    let due_future = due_jobs(&config, far_future).await.unwrap();
    assert_eq!(due_future.len(), 1, "job should be due in far future");
}

#[tokio::test]
async fn reschedule_after_run_persists_last_status_and_last_run() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let job = add_job(&config, "*/15 * * * *", "echo run").await.unwrap();
    reschedule_after_run(&config, &job, false, "failed output")
        .await
        .unwrap();

    let listed = list_jobs(&config).await.unwrap();
    let stored = listed.iter().find(|j| j.id == job.id).unwrap();
    assert_eq!(stored.last_status.as_deref(), Some("error"));
    assert!(stored.last_run.is_some());
}
