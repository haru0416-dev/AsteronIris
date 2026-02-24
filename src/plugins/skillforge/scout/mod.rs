//! Scout — skill discovery from external sources.

pub mod clawhub;
pub mod github;
pub mod huggingface;

pub use clawhub::ClawHubScout;
pub use github::GitHubScout;
pub use huggingface::HuggingFaceScout;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

// -- ScoutSource --

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoutSource {
    GitHub,
    ClawHub,
    HuggingFace,
}

impl std::str::FromStr for ScoutSource {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "github" => Self::GitHub,
            "clawhub" => Self::ClawHub,
            "huggingface" | "hf" => Self::HuggingFace,
            _ => {
                tracing::warn!(source = s, "Unknown scout source, defaulting to GitHub");
                Self::GitHub
            }
        })
    }
}

// -- ScoutResult --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoutResult {
    pub name: String,
    pub url: String,
    pub description: String,
    pub stars: u64,
    pub language: Option<String>,
    pub updated_at: Option<DateTime<Utc>>,
    pub source: ScoutSource,
    /// Owner / org extracted from the URL or API response.
    pub owner: String,
    /// Whether the repo has a license file.
    pub has_license: bool,
}

// -- Scout trait --

pub trait Scout: Send + Sync {
    /// Discover candidate skills from the source.
    fn discover(&self) -> Pin<Box<dyn Future<Output = Result<Vec<ScoutResult>>> + Send + '_>>;
}

// -- Helpers --

/// Minimal percent-encoding for query strings (space -> +).
pub(crate) fn urlencoding(s: &str) -> String {
    s.replace(' ', "+").replace('&', "%26").replace('#', "%23")
}

pub(crate) fn owner_from_url(url: &str) -> String {
    let mut segments = url.split('/').filter(|segment| !segment.is_empty());
    while let Some(segment) = segments.next() {
        if segment == "github.com" {
            return segments.next().unwrap_or("unknown").to_string();
        }
    }
    "unknown".to_string()
}

/// Deduplicate scout results by URL (keeps first occurrence).
pub fn dedup(results: &mut Vec<ScoutResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert(r.url.clone()));
}

// -- Tests --

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scout_source_from_str() {
        assert_eq!(
            "github".parse::<ScoutSource>().unwrap(),
            ScoutSource::GitHub
        );
        assert_eq!(
            "GitHub".parse::<ScoutSource>().unwrap(),
            ScoutSource::GitHub
        );
        assert_eq!(
            "clawhub".parse::<ScoutSource>().unwrap(),
            ScoutSource::ClawHub
        );
        assert_eq!(
            "huggingface".parse::<ScoutSource>().unwrap(),
            ScoutSource::HuggingFace
        );
        assert_eq!(
            "hf".parse::<ScoutSource>().unwrap(),
            ScoutSource::HuggingFace
        );
        // unknown falls back to GitHub
        assert_eq!(
            "unknown".parse::<ScoutSource>().unwrap(),
            ScoutSource::GitHub
        );
    }

    #[test]
    fn dedup_removes_duplicates() {
        let mut results = vec![
            ScoutResult {
                name: "a".into(),
                url: "https://github.com/x/a".into(),
                description: String::new(),
                stars: 10,
                language: None,
                updated_at: None,
                source: ScoutSource::GitHub,
                owner: "x".into(),
                has_license: true,
            },
            ScoutResult {
                name: "a-dup".into(),
                url: "https://github.com/x/a".into(),
                description: String::new(),
                stars: 10,
                language: None,
                updated_at: None,
                source: ScoutSource::GitHub,
                owner: "x".into(),
                has_license: true,
            },
            ScoutResult {
                name: "b".into(),
                url: "https://github.com/x/b".into(),
                description: String::new(),
                stars: 5,
                language: None,
                updated_at: None,
                source: ScoutSource::GitHub,
                owner: "x".into(),
                has_license: false,
            },
        ];
        dedup(&mut results);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "a");
        assert_eq!(results[1].name, "b");
    }

    #[test]
    fn urlencoding_works() {
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(urlencoding("a&b#c"), "a%26b%23c");
    }
}
