//! Example: Implementing a custom Channel for AsteronIris
//!
//! Channels let AsteronIris communicate through any messaging platform.
//! The trait uses `Pin<Box<dyn Future<...> + Send>>` for dyn-safety,
//! so channels are stored as `Arc<dyn Channel>`.
//!
//! Run: `cargo run --example custom_channel`

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use tokio::sync::mpsc;

// ── Minimal types (mirrors src/transport/channels/traits.rs) ────────

#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub channel: String,
    pub timestamp: u64,
}

// ── Minimal Channel trait ───────────────────────────────────────────

pub trait Channel: Send + Sync {
    fn name(&self) -> &str;

    fn send<'a>(
        &'a self,
        message: &'a str,
        recipient: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    fn listen<'a>(
        &'a self,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    fn health_check(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>>;
}

// ── Example: Telegram channel via Bot API ───────────────────────────

pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl TelegramChannel {
    pub fn new(bot_token: impl Into<String>, allowed_users: Vec<String>) -> Self {
        Self {
            bot_token: bot_token.into(),
            allowed_users,
            client: reqwest::Client::new(),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{method}", self.bot_token)
    }
}

impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    fn send<'a>(
        &'a self,
        message: &'a str,
        chat_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            self.client
                .post(self.api_url("sendMessage"))
                .json(&serde_json::json!({
                    "chat_id": chat_id,
                    "text": message,
                    "parse_mode": "Markdown",
                }))
                .send()
                .await?;
            Ok(())
        })
    }

    fn listen<'a>(
        &'a self,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut offset: i64 = 0;

            loop {
                let resp = self
                    .client
                    .get(self.api_url("getUpdates"))
                    .query(&[("offset", offset.to_string()), ("timeout", "30".into())])
                    .send()
                    .await?
                    .json::<serde_json::Value>()
                    .await?;

                if let Some(updates) = resp["result"].as_array() {
                    for update in updates {
                        if let Some(msg) = update.get("message") {
                            let sender = msg["from"]["username"]
                                .as_str()
                                .unwrap_or("unknown")
                                .to_string();

                            if !self.allowed_users.is_empty()
                                && !self.allowed_users.contains(&sender)
                            {
                                continue;
                            }

                            let channel_msg = ChannelMessage {
                                id: msg["message_id"].to_string(),
                                sender,
                                content: msg["text"].as_str().unwrap_or("").to_string(),
                                channel: "telegram".into(),
                                timestamp: msg["date"].as_u64().unwrap_or(0),
                            };

                            if tx.send(channel_msg).await.is_err() {
                                return Ok(());
                            }
                        }
                        offset = update["update_id"].as_i64().unwrap_or(offset) + 1;
                    }
                }
            }
        })
    }

    fn health_check(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            self.client
                .get(self.api_url("getMe"))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        })
    }
}

// ── Demo ─────────────────────────────────────────────────────────────

fn main() {
    // Channels are stored as `Arc<dyn Channel>` — the trait is dyn-safe.
    let _channel: Box<dyn Channel> = Box::new(TelegramChannel::new("BOT_TOKEN", vec![]));
    println!("Channel registered: telegram");
    println!("Add your channel config to ChannelsConfig in src/config/schema/");
}
