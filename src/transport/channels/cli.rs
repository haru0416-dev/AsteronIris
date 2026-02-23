use super::traits::{Channel, ChannelMessage};
use std::future::Future;
use std::pin::Pin;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use uuid::Uuid;

/// CLI channel — stdin/stdout, always available, zero deps
pub struct CliChannel;

impl CliChannel {
    pub fn new() -> Self {
        Self
    }
}

impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    fn max_message_length(&self) -> usize {
        usize::MAX
    }

    fn send<'a>(
        &'a self,
        message: &'a str,
        _recipient: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("{message}");
            Ok(())
        })
    }

    fn listen<'a>(
        &'a self,
        tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let stdin = io::stdin();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                if line == "/quit" || line == "/exit" {
                    break;
                }

                let msg = ChannelMessage {
                    id: Uuid::new_v4().to_string(),
                    sender: "user".to_string(),
                    content: line,
                    channel: "cli".to_string(),
                    conversation_id: None,
                    thread_id: None,
                    reply_to: None,
                    message_id: None,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    attachments: Vec::new(),
                };

                if tx.send(msg).await.is_err() {
                    break;
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_channel_name() {
        assert_eq!(CliChannel::new().name(), "cli");
    }

    #[tokio::test]
    async fn cli_channel_send_does_not_panic() {
        let ch = CliChannel::new();
        let result = ch.send("hello", "user").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_channel_send_empty_message() {
        let ch = CliChannel::new();
        let result = ch.send("", "").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn cli_channel_health_check() {
        let ch = CliChannel::new();
        assert!(ch.health_check().await);
    }

    #[test]
    fn channel_message_struct() {
        let msg = ChannelMessage {
            id: "test-id".into(),
            sender: "user".into(),
            content: "hello".into(),
            channel: "cli".into(),
            conversation_id: None,
            thread_id: None,
            reply_to: None,
            message_id: None,
            timestamp: 1_234_567_890,
            attachments: Vec::new(),
        };
        assert_eq!(msg.id, "test-id");
        assert_eq!(msg.sender, "user");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.channel, "cli");
        assert_eq!(msg.timestamp, 1_234_567_890);
    }

    #[test]
    fn channel_message_clone() {
        let msg = ChannelMessage {
            id: "id".into(),
            sender: "s".into(),
            content: "c".into(),
            channel: "ch".into(),
            conversation_id: None,
            thread_id: None,
            reply_to: None,
            message_id: None,
            timestamp: 0,
            attachments: Vec::new(),
        };
        let cloned = msg.clone();
        assert_eq!(cloned.id, msg.id);
        assert_eq!(cloned.content, msg.content);
    }
}
