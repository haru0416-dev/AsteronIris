mod expression;
mod repository;
mod types;

pub mod scheduler;

pub use repository::{
    add_job, add_job_with_metadata, due_jobs, list_jobs, remove_job, reschedule_after_run,
};
#[allow(unused_imports)]
pub use types::AGENT_PENDING_CAP;
pub use types::{CronJob, CronJobKind, CronJobMetadata, CronJobOrigin};

use crate::config::Config;
use anyhow::Result;

/// Cron management commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronCommand {
    List,
    Add { expression: String, command: String },
    Remove { id: String },
}

pub async fn handle_command(command: CronCommand, config: &Config) -> Result<()> {
    match command {
        CronCommand::List => {
            let jobs = list_jobs(config).await?;
            if jobs.is_empty() {
                println!("No scheduled tasks yet.");
                println!("\nUsage:");
                println!("  asteroniris cron add '0 9 * * *' 'agent -m \"Good morning!\"'");
                return Ok(());
            }

            println!("Scheduled jobs ({}):", jobs.len());
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
        CronCommand::Add {
            expression,
            command,
        } => {
            let job = add_job(config, &expression, &command).await?;
            println!("Added cron job {}", job.id);
            println!("  Expr: {}", job.expression);
            println!("  Next: {}", job.next_run.to_rfc3339());
            println!("  Cmd : {}", job.command);
            Ok(())
        }
        CronCommand::Remove { id } => remove_job(config, &id).await,
    }
}

#[cfg(test)]
mod tests;
