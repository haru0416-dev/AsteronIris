//! `HuggingFace` skill scout.

use super::{Scout, ScoutResult, ScoutSource, dedup, urlencoding};
use anyhow::{Context, Result};
use std::future::Future;
use std::pin::Pin;
use tracing::{debug, warn};

pub struct HuggingFaceScout {
    client: reqwest::Client,
    queries: Vec<String>,
    api_base: String,
}

impl HuggingFaceScout {
    pub fn new() -> Result<Self> {
        use std::time::Duration;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(anyhow::Error::from)
            .context("Failed to build HTTP client")?;

        let api_base = std::env::var("ASTERONIRIS_SKILLFORGE_HF_API_BASE")
            .unwrap_or_else(|_| "https://huggingface.co".to_string());

        Ok(Self {
            client,
            queries: vec!["asteroniris skill".into()],
            api_base,
        })
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
                    .and_then(|s| s.parse::<chrono::DateTime<chrono::Utc>>().ok());

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

impl Scout for HuggingFaceScout {
    fn discover(&self) -> Pin<Box<dyn Future<Output = Result<Vec<ScoutResult>>> + Send + '_>> {
        Box::pin(async move {
            let mut all: Vec<ScoutResult> = Vec::with_capacity(self.queries.len() * 30);

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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
