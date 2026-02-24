//! `ClawHub` skill scout.

use super::{Scout, ScoutResult, ScoutSource, dedup, owner_from_url, urlencoding};
use anyhow::{Context, Result};
use std::future::Future;
use std::pin::Pin;
use tracing::debug;

pub struct ClawHubScout {
    client: reqwest::Client,
    queries: Vec<String>,
    base_url: String,
}

impl ClawHubScout {
    pub fn new(base_url: Option<&str>, token: Option<&str>) -> Result<Self> {
        use std::time::Duration;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
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

        let base_url = base_url
            .map(str::to_string)
            .or_else(|| std::env::var("CLAWHUB_API_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.clawhub.com".to_string());

        Ok(Self {
            client,
            queries: vec!["asteroniris skill".into(), "ai agent skill".into()],
            base_url: base_url.trim_end_matches('/').to_string(),
        })
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
            .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

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

impl Scout for ClawHubScout {
    fn discover(&self) -> Pin<Box<dyn Future<Output = Result<Vec<ScoutResult>>> + Send + '_>> {
        Box::pin(async move {
            let mut all: Vec<ScoutResult> = Vec::with_capacity(self.queries.len() * 30);

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
                    .context("ClawHub API request failed")?;

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
                    .context("Failed to parse ClawHub response")?;

                let mut items = Self::parse_items(&body);
                debug!(
                    count = items.len(),
                    query = query.as_str(),
                    "Parsed ClawHub items"
                );
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
}
