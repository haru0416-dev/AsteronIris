use super::types::{MediaFile, MediaType};
use anyhow::Result;

pub struct MediaProcessor;

impl MediaProcessor {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub async fn describe(&self, file: &MediaFile, data: &[u8]) -> Result<String> {
        tokio::task::yield_now().await;
        let description = match file.media_type {
            MediaType::Image => Self::describe_image(file, data),
            MediaType::Audio => Self::transcribe_audio(file, data),
            MediaType::Video => "[Video content - processing not yet supported]".into(),
            MediaType::Document => "[Document content - processing not yet supported]".into(),
            MediaType::Unknown => "[Unknown media type]".into(),
        };
        Ok(description)
    }

    fn describe_image(file: &MediaFile, _data: &[u8]) -> String {
        format!(
            "[Image: {} ({}, {} bytes) - vision model not yet configured]",
            file.filename.as_deref().unwrap_or("unnamed"),
            file.mime_type,
            file.size_bytes,
        )
    }

    fn transcribe_audio(file: &MediaFile, _data: &[u8]) -> String {
        format!(
            "[Audio: {} ({}, {} bytes) - transcription not yet configured]",
            file.filename.as_deref().unwrap_or("unnamed"),
            file.mime_type,
            file.size_bytes
        )
    }
}
