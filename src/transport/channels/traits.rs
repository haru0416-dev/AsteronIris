use std::future::Future;
use std::pin::Pin;

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

/// A message received from or sent to a channel.
///
/// `sender` identifies the user (e.g. Discord user ID, Telegram user ID).
/// `conversation_id` identifies the conversation context (e.g. Discord channel/thread ID).
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub channel: String,
    pub conversation_id: Option<String>,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub message_id: Option<String>,
    pub timestamp: u64,
    pub attachments: Vec<MediaAttachment>,
}

/// Core channel trait — implement for any messaging platform
pub trait Channel: Send + Sync {
    /// Human-readable channel name
    fn name(&self) -> &str;

    /// Send a message through this channel
    fn send<'a>(
        &'a self,
        message: &'a str,
        recipient: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

    /// Start listening for incoming messages (long-running)
    fn listen<'a>(
        &'a self,
        tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>;

    /// Check if channel is healthy
    fn health_check<'a>(&'a self) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { true })
    }

    fn max_message_length(&self) -> usize {
        usize::MAX
    }

    fn send_typing<'a>(
        &'a self,
        _recipient: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    fn send_media<'a>(
        &'a self,
        _attachment: &'a MediaAttachment,
        _recipient: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move { anyhow::bail!("media sending not supported by this channel") })
    }

    fn edit_message<'a>(
        &'a self,
        _channel_id: &'a str,
        _message_id: &'a str,
        _content: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move { anyhow::bail!("message editing not supported by this channel") })
    }

    fn delete_message<'a>(
        &'a self,
        _channel_id: &'a str,
        _message_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move { anyhow::bail!("message deletion not supported by this channel") })
    }

    fn send_chunked<'a>(
        &'a self,
        message: &'a str,
        recipient: &'a str,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let chunks = super::chunker::chunk_message(message, self.max_message_length());
            for chunk in chunks {
                self.send(&chunk, recipient).await?;
            }
            Ok(())
        })
    }
}
