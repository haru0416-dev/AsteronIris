use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContent {
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfig {
    pub enabled: bool,
    pub max_links_per_message: usize,
    pub max_content_chars: usize,
    pub timeout_secs: u64,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_links_per_message: 3,
            max_content_chars: 2000,
            timeout_secs: 10,
        }
    }
}
