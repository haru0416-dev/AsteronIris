use clap::Subcommand;
use serde::{Deserialize, Serialize};

/// Service management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Channel management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChannelCommands {
    /// List all configured channels
    List,
    /// Start all configured channels (handled in main.rs for async)
    Start,
    /// Run health checks for configured channels (handled in main.rs for async)
    Doctor,
    /// Add a new channel configuration
    Add {
        /// Channel type (telegram, discord, slack, whatsapp, matrix, imessage, email)
        channel_type: String,
        /// Optional configuration as JSON
        config: String,
    },
    /// Remove a channel configuration
    Remove {
        /// Channel name to remove
        name: String,
    },
}

/// Skills management subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SkillCommands {
    /// List all installed skills
    List,
    /// Install a new skill from a URL or local path
    Install {
        /// Source URL or local path
        source: String,
    },
    /// Remove an installed skill
    Remove {
        /// Skill name to remove
        name: String,
    },
}

/// Cron subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Auth profile subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

/// Integration subcommands
#[derive(Subcommand, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntegrationCommands {
    /// Show details about a specific integration
    Info {
        /// Integration name
        name: String,
    },
}
