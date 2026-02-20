use super::types::Command;

pub fn parse_command(input: &str) -> Option<Command> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next()?.to_lowercase();
    let args = parts.next().unwrap_or("").trim();

    match cmd.as_str() {
        "/status" => Some(Command::Status),
        "/new" | "/reset" => Some(Command::New),
        "/compact" => Some(Command::Compact),
        "/think" => Some(Command::Think {
            level: if args.is_empty() {
                None
            } else {
                Some(args.to_string())
            },
        }),
        "/verbose" => Some(Command::Verbose),
        "/usage" => Some(Command::Usage),
        "/help" | "/?" => Some(Command::Help),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_command() {
        assert_eq!(parse_command("/status"), Some(Command::Status));
    }

    #[test]
    fn status_case_insensitive() {
        assert_eq!(parse_command("/STATUS"), Some(Command::Status));
    }

    #[test]
    fn help_command() {
        assert_eq!(parse_command("/help"), Some(Command::Help));
    }

    #[test]
    fn help_question_mark() {
        assert_eq!(parse_command("/?"), Some(Command::Help));
    }

    #[test]
    fn new_command() {
        assert_eq!(parse_command("/new"), Some(Command::New));
    }

    #[test]
    fn reset_alias() {
        assert_eq!(parse_command("/reset"), Some(Command::New));
    }

    #[test]
    fn think_with_level() {
        assert_eq!(
            parse_command("/think high"),
            Some(Command::Think {
                level: Some("high".to_string())
            })
        );
    }

    #[test]
    fn think_without_level() {
        assert_eq!(
            parse_command("/think"),
            Some(Command::Think { level: None })
        );
    }

    #[test]
    fn compact_command() {
        assert_eq!(parse_command("/compact"), Some(Command::Compact));
    }

    #[test]
    fn verbose_command() {
        assert_eq!(parse_command("/verbose"), Some(Command::Verbose));
    }

    #[test]
    fn usage_command() {
        assert_eq!(parse_command("/usage"), Some(Command::Usage));
    }

    #[test]
    fn plain_text_returns_none() {
        assert_eq!(parse_command("hello"), None);
    }

    #[test]
    fn unknown_command_returns_none() {
        assert_eq!(parse_command("/unknown"), None);
    }

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(parse_command(""), None);
    }

    #[test]
    fn status_ignores_extra_args() {
        assert_eq!(parse_command("/status extra args"), Some(Command::Status));
    }

    #[test]
    fn whitespace_only_returns_none() {
        assert_eq!(parse_command("   "), None);
    }

    #[test]
    fn leading_whitespace_accepted() {
        assert_eq!(parse_command("  /status"), Some(Command::Status));
    }
}
