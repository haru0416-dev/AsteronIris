use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use reqwest::{Method, Response, header::HeaderMap};
use serde_json::json;
use tokio::{sync::Mutex, time::sleep};

use super::types::API_BASE;

const MAX_RATE_LIMIT_RETRIES: u8 = 3;

#[derive(Debug, Clone)]
struct RateLimitBucket {
    remaining: u32,
    reset_at: f64,
}

pub struct DiscordHttpClient {
    client: reqwest::Client,
    bot_token: String,
    buckets: Arc<Mutex<HashMap<String, RateLimitBucket>>>,
    global_reset_at: Arc<Mutex<Option<f64>>>,
}

impl DiscordHttpClient {
    #[must_use]
    pub fn new(bot_token: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            bot_token: bot_token.into(),
            buckets: Arc::new(Mutex::new(HashMap::new())),
            global_reset_at: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<serde_json::Value> {
        let url = format!("{API_BASE}/channels/{channel_id}/messages");
        let response = self
            .request(Method::POST, &url, Some(json!({ "content": content })))
            .await
            .context("send Discord message")?;
        response
            .json()
            .await
            .context("parse Discord send message response JSON")
    }

    pub async fn send_embed(
        &self,
        channel_id: &str,
        title: Option<&str>,
        description: &str,
        color: Option<u32>,
    ) -> Result<()> {
        let url = format!("{API_BASE}/channels/{channel_id}/messages");
        let mut embed = json!({ "description": description });
        if let Some(embed_title) = title {
            embed["title"] = json!(embed_title);
        }
        if let Some(embed_color) = color {
            embed["color"] = json!(embed_color);
        }
        let _response = self
            .request(Method::POST, &url, Some(json!({ "embeds": [embed] })))
            .await
            .context("send Discord embed")?;
        Ok(())
    }

    pub async fn send_media(
        &self,
        channel_id: &str,
        bytes: Vec<u8>,
        filename: &str,
        mime_type: &str,
    ) -> Result<()> {
        let url = format!("{API_BASE}/channels/{channel_id}/messages");
        let filename_owned = filename.to_owned();
        let mime_type_owned = mime_type.to_owned();

        let route_key = Self::bucket_key_from_url(&url);
        self.wait_for_limits(&route_key).await;

        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let part = reqwest::multipart::Part::bytes(bytes.clone())
                .file_name(filename_owned.clone())
                .mime_str(&mime_type_owned)
                .context("set Discord media MIME type")?;
            let form = reqwest::multipart::Form::new().part("files[0]", part);

            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bot {}", self.bot_token))
                .multipart(form)
                .send()
                .await
                .context("send Discord media request")?;

            self.update_bucket_from_headers(&route_key, response.headers())
                .await;

            if response.status().as_u16() == 429 {
                if attempt == MAX_RATE_LIMIT_RETRIES {
                    anyhow::bail!(
                        "Discord media request exceeded rate limit after {MAX_RATE_LIMIT_RETRIES} retries"
                    );
                }
                let is_global = Self::is_global_limit(response.headers());
                let retry_after = Self::parse_retry_after(response.headers())
                    .unwrap_or_else(|| Duration::from_secs(1));
                self.handle_429_wait(is_global, retry_after, &route_key)
                    .await;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|error| format!("<failed to read response body: {error}>"));
                anyhow::bail!("Discord media request failed ({status}): {body}");
            }

            return Ok(());
        }

        anyhow::bail!("Discord media request failed due to rate limiting")
    }

    pub async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<()> {
        let url = format!("{API_BASE}/channels/{channel_id}/messages/{message_id}");
        let _response = self
            .request(Method::PATCH, &url, Some(json!({ "content": content })))
            .await
            .context("edit Discord message")?;
        Ok(())
    }

    pub async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<()> {
        let url = format!("{API_BASE}/channels/{channel_id}/messages/{message_id}");
        let _response = self
            .request(Method::DELETE, &url, None)
            .await
            .context("delete Discord message")?;
        Ok(())
    }

    pub async fn send_typing(&self, channel_id: &str) -> Result<()> {
        let url = format!("{API_BASE}/channels/{channel_id}/typing");
        let _response = self
            .request(Method::POST, &url, None)
            .await
            .context("send Discord typing indicator")?;
        Ok(())
    }

    pub async fn get_current_user(&self) -> Result<serde_json::Value> {
        let url = format!("{API_BASE}/users/@me");
        let response = self
            .request(Method::GET, &url, None)
            .await
            .context("fetch current Discord user")?;
        response
            .json()
            .await
            .context("parse current Discord user JSON")
    }

    pub async fn get_gateway_bot(&self) -> Result<serde_json::Value> {
        let url = format!("{API_BASE}/gateway/bot");
        let response = self
            .request(Method::GET, &url, None)
            .await
            .context("fetch Discord gateway bot data")?;
        response
            .json()
            .await
            .context("parse Discord gateway bot JSON")
    }

    pub async fn create_interaction_response(
        &self,
        interaction_id: &str,
        interaction_token: &str,
        response_type: u8,
        data: Option<serde_json::Value>,
    ) -> Result<()> {
        let url = format!("{API_BASE}/interactions/{interaction_id}/{interaction_token}/callback");
        let mut body = json!({ "type": response_type });
        if let Some(payload) = data {
            body["data"] = payload;
        }
        let _response = self
            .request(Method::POST, &url, Some(body))
            .await
            .context("create Discord interaction response")?;
        Ok(())
    }

    pub async fn edit_original_interaction_response(
        &self,
        application_id: &str,
        interaction_token: &str,
        content: &str,
    ) -> Result<()> {
        let url =
            format!("{API_BASE}/webhooks/{application_id}/{interaction_token}/messages/@original");
        let _response = self
            .request(Method::PATCH, &url, Some(json!({ "content": content })))
            .await
            .context("edit original Discord interaction response")?;
        Ok(())
    }

    pub async fn register_commands(
        &self,
        application_id: &str,
        guild_id: Option<&str>,
        commands: &[serde_json::Value],
    ) -> Result<()> {
        let url = if let Some(guild) = guild_id {
            format!("{API_BASE}/applications/{application_id}/guilds/{guild}/commands")
        } else {
            format!("{API_BASE}/applications/{application_id}/commands")
        };

        let _response = self
            .request(Method::PUT, &url, Some(json!(commands)))
            .await
            .context("register Discord application commands")?;
        Ok(())
    }

    async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response> {
        let route_key = Self::bucket_key_from_url(url);
        self.wait_for_limits(&route_key).await;

        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let mut request_builder = self
                .client
                .request(method.clone(), url)
                .header("Authorization", format!("Bot {}", self.bot_token));
            if let Some(payload) = body.clone() {
                request_builder = request_builder.json(&payload);
            }

            let response = request_builder
                .send()
                .await
                .with_context(|| format!("send Discord request {} {}", method.as_str(), url))?;

            self.update_bucket_from_headers(&route_key, response.headers())
                .await;

            if response.status().as_u16() == 429 {
                if attempt == MAX_RATE_LIMIT_RETRIES {
                    anyhow::bail!(
                        "Discord request {} {} exceeded rate limit after {} retries",
                        method.as_str(),
                        url,
                        MAX_RATE_LIMIT_RETRIES
                    );
                }
                let is_global = Self::is_global_limit(response.headers());
                let retry_after = Self::parse_retry_after(response.headers())
                    .unwrap_or_else(|| Duration::from_secs(1));
                self.handle_429_wait(is_global, retry_after, &route_key)
                    .await;
                continue;
            }

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|error| format!("<failed to read response body: {error}>"));
                anyhow::bail!(
                    "Discord request {} {} failed ({status}): {body_text}",
                    method.as_str(),
                    url
                );
            }

            return Ok(response);
        }

        anyhow::bail!(
            "Discord request {} {} failed due to rate limiting",
            method.as_str(),
            url
        )
    }

    fn parse_header_u32(headers: &HeaderMap, name: &str) -> Option<u32> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u32>().ok())
    }

    fn parse_header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<f64>().ok())
    }

    fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
        let seconds = Self::parse_header_f64(headers, "Retry-After")?;
        if seconds <= 0.0 {
            return Some(Duration::from_secs(0));
        }
        Some(Duration::from_secs_f64(seconds))
    }

    fn is_global_limit(headers: &HeaderMap) -> bool {
        headers
            .get("X-RateLimit-Global")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }

    fn now_unix_timestamp() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    fn bucket_key_from_url(url: &str) -> String {
        let path = reqwest::Url::parse(url)
            .map_or_else(|_| url.to_string(), |parsed| parsed.path().to_string());
        let path_without_api_prefix = path
            .strip_prefix("/api/v10")
            .map_or(path.as_str(), |stripped| stripped);

        let normalized = path_without_api_prefix
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                if segment.chars().all(|character| character.is_ascii_digit()) {
                    "{id}".to_string()
                } else {
                    segment.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("/");

        format!("/{normalized}")
    }

    async fn wait_for_limits(&self, route_key: &str) {
        let now = Self::now_unix_timestamp();
        let global_wait = {
            let global_guard = self.global_reset_at.lock().await;
            global_guard.and_then(|reset_at| (reset_at > now).then_some(reset_at - now))
        };
        if let Some(wait_secs) = global_wait {
            sleep(Duration::from_secs_f64(wait_secs)).await;
        }

        let route_wait = {
            let buckets = self.buckets.lock().await;
            buckets.get(route_key).and_then(|bucket| {
                if bucket.remaining == 0 && bucket.reset_at > now {
                    Some(bucket.reset_at - now)
                } else {
                    None
                }
            })
        };
        if let Some(wait_secs) = route_wait {
            sleep(Duration::from_secs_f64(wait_secs)).await;
        }
    }

    async fn handle_429_wait(&self, is_global: bool, retry_after: Duration, route_key: &str) {
        let now = Self::now_unix_timestamp();
        let reset_at = now + retry_after.as_secs_f64();
        if is_global {
            let mut global = self.global_reset_at.lock().await;
            *global = Some(reset_at);
        } else {
            let mut buckets = self.buckets.lock().await;
            buckets.insert(
                route_key.to_string(),
                RateLimitBucket {
                    remaining: 0,
                    reset_at,
                },
            );
        }
        sleep(retry_after).await;
    }

    async fn update_bucket_from_headers(&self, route_key: &str, headers: &HeaderMap) {
        let _limit = Self::parse_header_u32(headers, "X-RateLimit-Limit");
        let remaining = Self::parse_header_u32(headers, "X-RateLimit-Remaining");
        let reset_at = Self::parse_header_f64(headers, "X-RateLimit-Reset");
        let _bucket = headers.get("X-RateLimit-Bucket");

        if let (Some(remaining), Some(reset_at)) = (remaining, reset_at) {
            let mut buckets = self.buckets.lock().await;
            buckets.insert(
                route_key.to_string(),
                RateLimitBucket {
                    remaining,
                    reset_at,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DiscordHttpClient;

    #[test]
    fn extracts_rate_limit_bucket_key_from_url_path() {
        let url = "https://discord.com/api/v10/channels/123456789/messages";
        assert_eq!(
            DiscordHttpClient::bucket_key_from_url(url),
            "/channels/{id}/messages"
        );
    }

    #[test]
    fn parses_retry_after_float_seconds() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "Retry-After",
            reqwest::header::HeaderValue::from_static("1.75"),
        );

        let retry_after = DiscordHttpClient::parse_retry_after(&headers);
        assert!(retry_after.is_some());
        let duration = retry_after.unwrap_or_default();
        assert_eq!(duration.as_secs(), 1);
        assert_eq!(duration.subsec_millis(), 750);
    }

    #[tokio::test]
    async fn constructor_initializes_http_client_state() {
        let client = DiscordHttpClient::new("token");

        assert_eq!(client.bot_token, "token");
        assert!(client.buckets.lock().await.is_empty());
        assert!(client.global_reset_at.lock().await.is_none());
    }
}
