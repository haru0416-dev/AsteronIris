use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct MediaAttachment {
    pub mime_type: String,
    pub data: MediaData,
    pub filename: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MediaData {
    Url(String),
    Bytes(Vec<u8>),
}

/// A message received from or sent to a channel
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub channel: String,
    pub timestamp: u64,
    pub attachments: Vec<MediaAttachment>,
}

/// Core channel trait — implement for any messaging platform
#[async_trait]
pub trait Channel: Send + Sync {
    /// Human-readable channel name
    fn name(&self) -> &str;

    /// Send a message through this channel
    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()>;

    /// Start listening for incoming messages (long-running)
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()>;

    /// Check if channel is healthy
    async fn health_check(&self) -> bool {
        true
    }

    fn max_message_length(&self) -> usize {
        usize::MAX
    }

    async fn send_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn send_media(
        &self,
        _attachment: &MediaAttachment,
        _recipient: &str,
    ) -> anyhow::Result<()> {
        anyhow::bail!("media sending not supported by this channel")
    }

    async fn send_chunked(&self, message: &str, recipient: &str) -> anyhow::Result<()> {
        let chunks = super::chunker::chunk_message(message, self.max_message_length());
        for chunk in chunks {
            self.send(&chunk, recipient).await?;
        }
        Ok(())
    }
}
