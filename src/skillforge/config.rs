//! `SkillForgeConfig` — Configuration for the skill-forge pipeline.

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct SkillForgeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_auto_integrate")]
    pub auto_integrate: bool,
    #[serde(default = "default_sources")]
    pub sources: Vec<String>,
    #[serde(default = "default_scan_interval")]
    pub scan_interval_hours: u64,
    #[serde(default = "default_min_score")]
    pub min_score: f64,
    /// Optional GitHub personal-access token for higher rate limits.
    #[serde(default)]
    pub github_token: Option<String>,
    #[serde(default)]
    pub clawhub_token: Option<String>,
    #[serde(default)]
    pub clawhub_base_url: Option<String>,
    /// Directory where integrated skills are written.
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
}

fn default_auto_integrate() -> bool {
    true
}
fn default_sources() -> Vec<String> {
    vec!["github".into(), "clawhub".into()]
}
fn default_scan_interval() -> u64 {
    24
}
fn default_min_score() -> f64 {
    0.7
}
fn default_output_dir() -> String {
    "./skills".into()
}

impl Default for SkillForgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_integrate: default_auto_integrate(),
            sources: default_sources(),
            scan_interval_hours: default_scan_interval(),
            min_score: default_min_score(),
            github_token: None,
            clawhub_token: None,
            clawhub_base_url: None,
            output_dir: default_output_dir(),
        }
    }
}

impl std::fmt::Debug for SkillForgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillForgeConfig")
            .field("enabled", &self.enabled)
            .field("auto_integrate", &self.auto_integrate)
            .field("sources", &self.sources)
            .field("scan_interval_hours", &self.scan_interval_hours)
            .field("min_score", &self.min_score)
            .field("github_token", &self.github_token.as_ref().map(|_| "***"))
            .field("clawhub_token", &self.clawhub_token.as_ref().map(|_| "***"))
            .field("clawhub_base_url", &self.clawhub_base_url)
            .field("output_dir", &self.output_dir)
            .finish()
    }
}
