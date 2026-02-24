//! GitHub skill scout.

use super::{Scout, ScoutResult, ScoutSource, dedup, urlencoding};
use anyhow::{Context, Result};
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, warn};

/// Searches GitHub for repos matching skill-related queries.
pub struct GitHubScout {
    client: reqwest::Client,
    queries: Vec<String>,
}

impl GitHubScout {
    pub fn new(token: Option<&str>) -> Result<Self> {
        use std::time::Duration;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("AsteronIris-SkillForge/0.1"),
        );
        if let Some(t) = token
            && let Ok(val) = format!("Bearer {t}").parse()
        {
            headers.insert(reqwest::header::AUTHORIZATION, val);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(anyhow::Error::from)
            .context("Failed to build HTTP client")?;

        Ok(Self {
            client,
            queries: vec!["asteroniris skill".into(), "ai agent skill".into()],
        })
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
                    .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());
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

impl Scout for GitHubScout {
    fn discover(&self) -> Pin<Box<dyn Future<Output = Result<Vec<ScoutResult>>> + Send + '_>> {
        Box::pin(async move {
            let mut all: Vec<ScoutResult> = Vec::with_capacity(self.queries.len() * 30);

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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
