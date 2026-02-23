use clap::{Parser, Subcommand};

pub mod handlers;
pub mod parser;
mod subcommands;
pub mod types;

pub use handlers::handle_command;
pub use parser::parse_command;
pub use subcommands::{
    AuthCommands, ChannelCommands, CronCommands, IntegrationCommands, ServiceCommands,
    SkillCommands,
};
pub use types::{Command, CommandResult};

/// `AsteronIris` - Secure, extensible AI assistant built in Rust.
#[derive(Parser, Debug)]
#[command(name = "asteroniris")]
#[command(author = "theonlyhennygod")]
#[command(version = "0.1.0")]
#[command(about = "A secure, extensible AI assistant.", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize your workspace and configuration
    Onboard {
        /// Run the full interactive wizard (default is quick setup)
        #[arg(long)]
        interactive: bool,

        /// Reconfigure channels only (fast repair flow)
        #[arg(long)]
        channels_only: bool,

        /// API key (used in quick mode, ignored with --interactive)
        #[arg(long)]
        api_key: Option<String>,

        /// Provider name (used in quick mode, default: openrouter)
        #[arg(long)]
        provider: Option<String>,

        /// Memory backend (sqlite, markdown, none) - used in quick mode, default: sqlite
        #[arg(long)]
        memory: Option<String>,

        /// Also install the daemon as an OS service (launchd/systemd)
        #[arg(long)]
        install_daemon: bool,
    },

    /// Start the AI agent loop
    Agent {
        /// Single message mode (don't enter interactive mode)
        #[arg(short, long)]
        message: Option<String>,

        /// Provider to use (openrouter, anthropic, openai)
        #[arg(short, long)]
        provider: Option<String>,

        /// Model to use
        #[arg(long)]
        model: Option<String>,

        /// Temperature (0.0 - 2.0)
        #[arg(short, long, default_value = "0.7")]
        temperature: f64,
    },

    /// Start the gateway server (webhooks, websockets)
    Gateway {
        /// Port to listen on (use 0 for random available port)
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Start long-running autonomous runtime (gateway + channels + heartbeat + scheduler)
    Daemon {
        /// Port to listen on (use 0 for random available port)
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },

    /// Manage OS service lifecycle (launchd/systemd user service)
    Service {
        #[command(subcommand)]
        service_command: ServiceCommands,
    },

    /// Run diagnostics for daemon/scheduler/channel freshness
    Doctor,

    /// Show system status (full details)
    Status,

    /// Run deterministic evaluation harness baseline suites
    Eval {
        /// Deterministic RNG seed
        #[arg(long, default_value_t = 42)]
        seed: u64,

        /// Optional evidence slug (writes .sisyphus/evidence files when set)
        #[arg(long)]
        evidence_slug: Option<String>,
    },

    Evolve {
        #[arg(long)]
        apply: bool,
    },

    Model {
        #[arg(long)]
        set: String,

        #[arg(long)]
        provider: Option<String>,
    },

    /// Configure and manage scheduled tasks
    Cron {
        #[command(subcommand)]
        cron_command: CronCommands,
    },

    /// Manage channels (telegram, discord, slack)
    Channel {
        #[command(subcommand)]
        channel_command: ChannelCommands,
    },

    /// Browse 50+ integrations
    Integrations {
        #[command(subcommand)]
        integration_command: IntegrationCommands,
    },

    /// Manage auth profiles and credentials
    Auth {
        #[command(subcommand)]
        auth_command: AuthCommands,
    },

    /// Manage skills (user-defined capabilities)
    Skills {
        #[command(subcommand)]
        skill_command: SkillCommands,
    },
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands};
    use clap::CommandFactory;
    use clap::Parser;

    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_eval_command_with_seed_and_slug() {
        let cli = Cli::parse_from([
            "asteroniris",
            "eval",
            "--seed",
            "99",
            "--evidence-slug",
            "baseline",
        ]);

        match cli.command {
            Commands::Eval {
                seed,
                evidence_slug,
            } => {
                assert_eq!(seed, 99);
                assert_eq!(evidence_slug.as_deref(), Some("baseline"));
            }
            other => panic!("expected eval command, got {other:?}"),
        }
    }
}
