//! Scout — skill discovery from external sources.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// ScoutSource
// ---------------------------------------------------------------------------

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
                warn!(source = s, "Unknown scout source, defaulting to GitHub");
                Self::GitHub
            }
        })
    }
}

// ---------------------------------------------------------------------------
// ScoutResult
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Scout trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Scout: Send + Sync {
    /// Discover candidate skills from the source.
    async fn discover(&self) -> Result<Vec<ScoutResult>>;
}

// ---------------------------------------------------------------------------
// GitHubScout
// ---------------------------------------------------------------------------

/// Searches GitHub for repos matching skill-related queries.
pub struct GitHubScout {
    client: reqwest::Client,
    queries: Vec<String>,
}

pub struct HuggingFaceScout {
    client: reqwest::Client,
    queries: Vec<String>,
    api_base: String,
}

pub struct ClawHubScout {
    client: reqwest::Client,
    queries: Vec<String>,
    base_url: String,
}

impl GitHubScout {
    pub fn new(token: Option<&str>) -> Self {
        use std::time::Duration;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "application/vnd.github+json".parse().expect("valid header"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            "AsteronIris-SkillForge/0.1".parse().expect("valid header"),
        );
        if let Some(t) = token {
            if let Ok(val) = format!("Bearer {t}").parse() {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        Self {
            client,
            queries: vec!["asteroniris skill".into(), "ai agent skill".into()],
        }
    }

    /// Parse the GitHub search/repositories JSON response.
    fn parse_items(body: &serde_json::Value) -> Vec<ScoutResult> {
        let Some(items) = body.get("items").and_then(|v| v.as_array()) else {
            return vec![];
        };

        items
            .iter()
            .filter_map(|item| {
                let name = item.get("name")?.as_str()?.to_string();
                let url = item.get("html_url")?.as_str()?.to_string();
                let description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stars = item
                    .get("stargazers_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let language = item
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let updated_at = item
                    .get("updated_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());
                let owner = item
                    .get("owner")
                    .and_then(|o| o.get("login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let has_license = item.get("license").is_some_and(|v| !v.is_null());

                Some(ScoutResult {
                    name,
                    url,
                    description,
                    stars,
                    language,
                    updated_at,
                    source: ScoutSource::GitHub,
                    owner,
                    has_license,
                })
            })
            .collect()
    }
}

impl HuggingFaceScout {
    pub fn new() -> Self {
        use std::time::Duration;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        let api_base = std::env::var("ASTERONIRIS_SKILLFORGE_HF_API_BASE")
            .unwrap_or_else(|_| "https://huggingface.co".to_string());

        Self {
            client,
            queries: vec!["asteroniris skill".into()],
            api_base,
        }
    }

    fn parse_items(body: &serde_json::Value) -> Vec<ScoutResult> {
        let Some(items) = body.as_array() else {
            return vec![];
        };

        items
            .iter()
            .filter_map(|item| {
                let id = item.get("id").and_then(|v| v.as_str())?.to_string();
                let description = item
                    .get("cardData")
                    .and_then(|v| v.get("description"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let stars = item
                    .get("likes")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let tags: &[serde_json::Value] = item
                    .get("tags")
                    .and_then(serde_json::Value::as_array)
                    .map_or(&[] as &[_], std::vec::Vec::as_slice);

                let language = tags.iter().find_map(|tag| {
                    let raw = tag.as_str()?;
                    match raw.to_ascii_lowercase().as_str() {
                        "rust" => Some("Rust".to_string()),
                        "python" => Some("Python".to_string()),
                        "javascript" => Some("JavaScript".to_string()),
                        "typescript" => Some("TypeScript".to_string()),
                        _ => None,
                    }
                });

                let has_license = item
                    .get("cardData")
                    .and_then(|v| v.get("license"))
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
                    || tags.iter().any(|tag| {
                        tag.as_str()
                            .is_some_and(|s| s.to_ascii_lowercase().starts_with("license:"))
                    });

                let updated_at = item
                    .get("lastModified")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<DateTime<Utc>>().ok());

                let owner = id
                    .split_once('/')
                    .map_or_else(|| "unknown".to_string(), |(org, _)| org.to_string());

                Some(ScoutResult {
                    name: id.clone(),
                    url: format!("https://huggingface.co/{id}"),
                    description,
                    stars,
                    language,
                    updated_at,
                    source: ScoutSource::HuggingFace,
                    owner,
                    has_license,
                })
            })
            .collect()
    }
}

impl ClawHubScout {
    pub fn new(base_url: Option<&str>, token: Option<&str>) -> Self {
        use std::time::Duration;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            "application/json".parse().expect("valid header"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            "AsteronIris-SkillForge/0.1".parse().expect("valid header"),
        );
        if let Some(t) = token {
            if let Ok(val) = format!("Bearer {t}").parse() {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        let base_url = base_url
            .map(str::to_string)
            .or_else(|| std::env::var("CLAWHUB_API_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.clawhub.com".to_string());

        Self {
            client,
            queries: vec!["asteroniris skill".into(), "ai agent skill".into()],
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    fn parse_items(body: &serde_json::Value) -> Vec<ScoutResult> {
        let items = body
            .get("items")
            .and_then(|v| v.as_array())
            .or_else(|| body.get("results").and_then(|v| v.as_array()))
            .or_else(|| body.get("data").and_then(|v| v.as_array()))
            .or_else(|| body.as_array());

        let Some(items) = items else {
            return vec![];
        };

        items.iter().filter_map(Self::parse_item).collect()
    }

    fn parse_item(item: &serde_json::Value) -> Option<ScoutResult> {
        let name = item
            .get("name")
            .or_else(|| item.get("repo_name"))
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())?
            .to_string();

        let url = item
            .get("url")
            .or_else(|| item.get("html_url"))
            .or_else(|| item.get("repository_url"))
            .or_else(|| item.get("repo_url"))
            .and_then(|v| v.as_str())?
            .to_string();

        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let stars = item
            .get("stars")
            .or_else(|| item.get("stargazers_count"))
            .or_else(|| item.get("star_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        let language = item
            .get("language")
            .or_else(|| item.get("primary_language"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let updated_at = item
            .get("updated_at")
            .or_else(|| item.get("updatedAt"))
            .or_else(|| item.get("last_updated"))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());

        let owner = item
            .get("owner")
            .and_then(|o| {
                o.get("login")
                    .or_else(|| o.get("name"))
                    .or_else(|| o.get("id"))
            })
            .and_then(|v| v.as_str())
            .map_or_else(|| owner_from_url(&url), String::from);

        let has_license = item
            .get("has_license")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_else(|| {
                item.get("license")
                    .is_some_and(|license| !license.is_null() && license != false)
            });

        Some(ScoutResult {
            name,
            url,
            description,
            stars,
            language,
            updated_at,
            source: ScoutSource::ClawHub,
            owner,
            has_license,
        })
    }
}

#[async_trait]
impl Scout for GitHubScout {
    async fn discover(&self) -> Result<Vec<ScoutResult>> {
        let mut all: Vec<ScoutResult> = Vec::new();

        for query in &self.queries {
            let url = format!(
                "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page=30",
                urlencoding(query)
            );
            debug!(query = query.as_str(), "Searching GitHub");

            let resp = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        query = query.as_str(),
                        error = %e,
                        "GitHub API request failed, skipping query"
                    );
                    continue;
                }
            };

            if !resp.status().is_success() {
                warn!(
                    status = %resp.status(),
                    query = query.as_str(),
                    "GitHub search returned non-200"
                );
                continue;
            }

            let body: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        query = query.as_str(),
                        error = %e,
                        "Failed to parse GitHub response, skipping query"
                    );
                    continue;
                }
            };

            let mut items = Self::parse_items(&body);
            debug!(count = items.len(), query = query.as_str(), "Parsed items");
            all.append(&mut items);
        }

        dedup(&mut all);
        Ok(all)
    }
}

#[async_trait]
impl Scout for HuggingFaceScout {
    async fn discover(&self) -> Result<Vec<ScoutResult>> {
        let mut all: Vec<ScoutResult> = Vec::new();

        for query in &self.queries {
            let url = format!(
                "{}/api/models?search={}&limit=30",
                self.api_base,
                urlencoding(query)
            );
            debug!(query = query.as_str(), "Searching HuggingFace");

            let resp = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    warn!(
                        query = query.as_str(),
                        error = %e,
                        "HuggingFace API request failed, skipping query"
                    );
                    continue;
                }
            };

            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                warn!(
                    query = query.as_str(),
                    "HuggingFace rate-limited request, skipping query"
                );
                continue;
            }

            if !resp.status().is_success() {
                warn!(
                    status = %resp.status(),
                    query = query.as_str(),
                    "HuggingFace search returned non-200"
                );
                continue;
            }

            let body: serde_json::Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        query = query.as_str(),
                        error = %e,
                        "Failed to parse HuggingFace response, skipping query"
                    );
                    continue;
                }
            };

            let mut items = Self::parse_items(&body);
            debug!(
                count = items.len(),
                query = query.as_str(),
                "Parsed HuggingFace items"
            );
            all.append(&mut items);
        }

        dedup(&mut all);
        Ok(all)
    }
}

#[async_trait]
impl Scout for ClawHubScout {
    async fn discover(&self) -> Result<Vec<ScoutResult>> {
        let mut all: Vec<ScoutResult> = Vec::new();

        for query in &self.queries {
            let url = format!(
                "{}/v1/skills?q={}&per_page=30",
                self.base_url,
                urlencoding(query)
            );
            debug!(query = query.as_str(), "Searching ClawHub");

            let resp = self
                .client
                .get(&url)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("ClawHub API request failed: {e}"))?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                if status == reqwest::StatusCode::UNAUTHORIZED {
                    anyhow::bail!("ClawHub authentication failed (401): {body}");
                }
                anyhow::bail!("ClawHub search failed ({status}): {body}");
            }

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to parse ClawHub response: {e}"))?;

            let mut items = Self::parse_items(&body);
            debug!(count = items.len(), query = query.as_str(), "Parsed ClawHub items");
            all.append(&mut items);
        }

        dedup(&mut all);
        Ok(all)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Minimal percent-encoding for query strings (space → +).
fn urlencoding(s: &str) -> String {
    s.replace(' ', "+").replace('&', "%26").replace('#', "%23")
}

fn owner_from_url(url: &str) -> String {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    fn parse_github_items() {
        let json = serde_json::json!({
            "total_count": 1,
            "items": [
                {
                    "name": "cool-skill",
                    "html_url": "https://github.com/user/cool-skill",
                    "description": "A cool skill",
                    "stargazers_count": 42,
                    "language": "Rust",
                    "updated_at": "2026-01-15T10:00:00Z",
                    "owner": { "login": "user" },
                    "license": { "spdx_id": "MIT" }
                }
            ]
        });
        let items = GitHubScout::parse_items(&json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "cool-skill");
        assert_eq!(items[0].stars, 42);
        assert!(items[0].has_license);
        assert_eq!(items[0].owner, "user");
    }

    #[test]
    fn parse_huggingface_items() {
        let json = serde_json::json!([
            {
                "id": "org/test-skill",
                "cardData": {
                    "description": "HF skill",
                    "license": "apache-2.0"
                },
                "likes": 7,
                "tags": ["rust", "license:apache-2.0"],
                "lastModified": "2026-01-15T10:00:00Z"
            }
        ]);

        let items = HuggingFaceScout::parse_items(&json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "org/test-skill");
        assert_eq!(items[0].url, "https://huggingface.co/org/test-skill");
        assert_eq!(items[0].owner, "org");
        assert_eq!(items[0].source, ScoutSource::HuggingFace);
        assert!(items[0].has_license);
        assert_eq!(items[0].language.as_deref(), Some("Rust"));
        assert_eq!(items[0].stars, 7);
    }

    #[test]
    fn parse_clawhub_items_maps_shared_schema() {
        let json = serde_json::json!({
            "results": [
                {
                    "name": "claw-skill",
                    "url": "https://github.com/claw-org/claw-skill",
                    "description": "ClawHub-origin skill",
                    "stars": 21,
                    "language": "Rust",
                    "updated_at": "2026-01-15T10:00:00Z",
                    "owner": { "login": "claw-org" },
                    "has_license": true
                }
            ]
        });

        let items = ClawHubScout::parse_items(&json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "claw-skill");
        assert_eq!(items[0].source, ScoutSource::ClawHub);
        assert_eq!(items[0].owner, "claw-org");
        assert!(items[0].has_license);
        assert_eq!(items[0].stars, 21);
    }

    #[test]
    fn parse_clawhub_items_derives_owner_from_url() {
        let json = serde_json::json!({
            "items": [
                {
                    "name": "claw-skill",
                    "url": "https://github.com/fallback-owner/claw-skill"
                }
            ]
        });

        let items = ClawHubScout::parse_items(&json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].owner, "fallback-owner");
        assert!(!items[0].has_license);
    }

    #[test]
    fn urlencoding_works() {
        assert_eq!(urlencoding("hello world"), "hello+world");
        assert_eq!(urlencoding("a&b#c"), "a%26b%23c");
    }
}
