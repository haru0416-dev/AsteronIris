use super::types::{Command, CommandResult};

pub fn handle_command(command: &Command) -> CommandResult {
    match command {
        Command::Status => handle_status(),
        Command::New => handle_new(),
        Command::Compact => handle_compact(),
        Command::Think { level } => handle_think(level.as_deref()),
        Command::Verbose => handle_verbose(),
        Command::Usage => handle_usage(),
        Command::Help => handle_help(),
    }
}

fn handle_status() -> CommandResult {
    CommandResult::visible("AsteronIris is running.")
}

fn handle_new() -> CommandResult {
    CommandResult::visible("Session reset. Starting fresh.")
}

fn handle_compact() -> CommandResult {
    CommandResult::visible("Session compacted.")
}

fn handle_think(level: Option<&str>) -> CommandResult {
    match level {
        Some(l) => CommandResult::ephemeral(format!("Thinking level set to: {l}")),
        None => CommandResult::ephemeral("Thinking level toggled."),
    }
}

fn handle_verbose() -> CommandResult {
    CommandResult::ephemeral("Verbose mode toggled.")
}

fn handle_usage() -> CommandResult {
    CommandResult::visible("Usage tracking not yet configured.")
}

fn handle_help() -> CommandResult {
    CommandResult::visible(
        "/status  -- Show current status\n\
         /new     -- Start a new session\n\
         /compact -- Summarize session history\n\
         /think   -- Toggle thinking mode (e.g. /think high)\n\
         /verbose -- Toggle verbose output\n\
         /usage   -- Show token usage statistics\n\
         /help    -- Show this help message",
    )
}
