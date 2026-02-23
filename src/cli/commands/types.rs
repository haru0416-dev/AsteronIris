use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Status,
    New,
    Compact,
    Think { level: Option<String> },
    Verbose,
    Usage,
    Help,
}

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub text: String,
    pub ephemeral: bool,
}

impl CommandResult {
    pub fn visible(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ephemeral: false,
        }
    }

    pub fn ephemeral(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ephemeral: true,
        }
    }
}
