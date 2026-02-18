use clap::{Parser, Subcommand};

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
pub enum ServiceCommands {
    /// Install daemon service unit for auto-start and restart
    Install,
    /// Start daemon service
    Start,
    /// Stop daemon service
    Stop,
    /// Check daemon service status
    Status,
    /// Uninstall daemon service unit
    Uninstall,
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

#[derive(Subcommand, Debug)]
pub enum CronCommands {
    /// List all scheduled tasks
    List,
    /// Add a new scheduled task
    Add {
        /// Cron expression
        expression: String,
        /// Command to run
        command: String,
    },
    /// Remove a scheduled task
    Remove {
        /// Task ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ChannelCommands {
    /// List configured channels
    List,
    /// Start all configured channels (Telegram, Discord, Slack)
    Start,
    /// Run health checks for configured channels
    Doctor,
    /// Add a new channel
    Add {
        /// Channel type
        channel_type: String,
        /// Configuration JSON
        config: String,
    },
    /// Remove a channel
    Remove {
        /// Channel name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillCommands {
    /// List installed skills
    List,
    /// Install a skill from a GitHub URL or local path
    Install {
        /// GitHub URL or local path
        source: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum IntegrationCommands {
    /// Show details about a specific integration
    Info {
        /// Integration name
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AuthCommands {
    /// List configured auth profiles
    List,
    /// Show auth status for a provider
    Status {
        /// Provider to inspect (defaults to configured default provider)
        #[arg(short, long)]
        provider: Option<String>,
    },
    /// Save or update an API-key auth profile
    Login {
        /// Provider name (e.g. openrouter, openai, anthropic)
        #[arg(short, long)]
        provider: String,
        /// Profile id (defaults to <provider>-default)
        #[arg(long)]
        profile: Option<String>,
        /// Human label for the profile
        #[arg(long)]
        label: Option<String>,
        /// API key to store (if omitted, prompt securely)
        #[arg(long)]
        api_key: Option<String>,
        /// Do not set this profile as provider default
        #[arg(long)]
        no_default: bool,
    },
    /// Login using OAuth via provider CLI and store imported token profile
    #[command(name = "oauth-login")]
    OAuthLogin {
        /// OAuth source/provider (codex/openai or claude/anthropic)
        #[arg(short, long)]
        provider: String,
        /// Profile id (defaults to <provider>-oauth-default)
        #[arg(long)]
        profile: Option<String>,
        /// Human label for the profile
        #[arg(long)]
        label: Option<String>,
        /// Do not set this profile as provider default
        #[arg(long)]
        no_default: bool,
        /// Skip launching provider login CLI and import from local credentials only
        #[arg(long)]
        skip_cli_login: bool,
        /// Claude setup token (sk-ant-oat01-...), if already obtained
        #[arg(long)]
        setup_token: Option<String>,
    },
    /// Show OAuth source health (codex/claude)
    #[command(name = "oauth-status")]
    OAuthStatus {
        /// OAuth source/provider to inspect (codex or claude)
        #[arg(short, long)]
        provider: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_has_no_flag_conflicts() {
        Cli::command().debug_assert();
    }
}
