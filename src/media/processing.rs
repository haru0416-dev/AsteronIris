use super::types::{MediaFile, MediaType};
use anyhow::Result;
use std::sync::Arc;

use crate::core::providers::Provider;
use crate::core::providers::response::{ContentBlock, ImageSource, ProviderMessage};

const IMAGE_DESCRIPTION_PROMPT: &str = "Describe this image concisely in 1-2 sentences.";

pub struct MediaProcessor {
    provider: Option<Arc<dyn Provider>>,
    model: Option<String>,
}

impl MediaProcessor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            provider: None,
            model: None,
        }
    }

    #[must_use]
    pub fn with_provider(provider: Arc<dyn Provider>, model: String) -> Self {
        Self {
            provider: Some(provider),
            model: Some(model),
        }
    }

    pub async fn describe(&self, file: &MediaFile, data: &[u8]) -> Result<String> {
        let description = match file.media_type {
            MediaType::Image => self.describe_image(file, data).await,
            MediaType::Audio => Self::transcribe_audio(file),
            MediaType::Video => "[Video content - processing not yet supported]".into(),
            MediaType::Document => Self::describe_document(file, data),
            MediaType::Unknown => "[Unknown media type]".into(),
        };
        Ok(description)
    }

    async fn describe_image(&self, file: &MediaFile, data: &[u8]) -> String {
        if let Some(provider) = &self.provider
            && provider.supports_vision()
            && let Some(model) = self.model.as_deref()
        {
            let source = ImageSource::base64(&file.mime_type, encode_base64(data));
            let messages = vec![ProviderMessage::user_with_image(
                "Describe this image.",
                source,
            )];

            match provider
                .chat_with_tools(Some(IMAGE_DESCRIPTION_PROMPT), &messages, &[], model, 0.2)
                .await
            {
                Ok(response) => {
                    if let Some(text) = extract_response_text(&response) {
                        return text;
                    }
                }
                Err(error) => {
                    tracing::debug!(
                        filename = ?file.filename,
                        mime_type = %file.mime_type,
                        error = %error,
                        "vision description failed; using image metadata fallback"
                    );
                }
            }
        }

        Self::image_metadata(file)
    }

    fn transcribe_audio(file: &MediaFile) -> String {
        let duration_secs = estimate_audio_duration_secs(file.size_bytes, &file.mime_type);
        let duration = format_duration(duration_secs);
        format!(
            "[Audio: {} ({}, {} bytes, ~{} estimated)]",
            file.filename.as_deref().unwrap_or("unnamed"),
            file.mime_type,
            file.size_bytes,
            duration,
        )
    }

    fn describe_document(file: &MediaFile, data: &[u8]) -> String {
        let filename = file.filename.as_deref().unwrap_or("unnamed");
        if file.mime_type.starts_with("text/") {
            return match std::str::from_utf8(data) {
                Ok(text) => {
                    let preview: String = text.chars().take(500).collect();
                    if preview.is_empty() {
                        format!(
                            "[Document: {filename} ({}, {} bytes)]",
                            file.mime_type, file.size_bytes
                        )
                    } else {
                        format!(
                            "[Document: {filename} ({}, {} bytes)] Preview: {}",
                            file.mime_type, file.size_bytes, preview
                        )
                    }
                }
                Err(_) => format!(
                    "[Document: {filename} ({}, {} bytes)]",
                    file.mime_type, file.size_bytes
                ),
            };
        }

        if file.mime_type == "application/pdf" {
            return format!("[PDF document: {filename} ({} bytes)]", file.size_bytes);
        }

        format!(
            "[Document: {filename} ({}, {} bytes)]",
            file.mime_type, file.size_bytes
        )
    }

    fn image_metadata(file: &MediaFile) -> String {
        format!(
            "[Image: {} ({}, {} bytes)]",
            file.filename.as_deref().unwrap_or("unnamed"),
            file.mime_type,
            file.size_bytes,
        )
    }
}

fn extract_response_text(
    response: &crate::core::providers::response::ProviderResponse,
) -> Option<String> {
    let text = response.text.trim();
    if !text.is_empty() {
        return Some(text.to_string());
    }

    response.content_blocks.iter().find_map(|block| {
        if let ContentBlock::Text { text } = block {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        None
    })
}

fn estimate_audio_duration_secs(size_bytes: u64, mime_type: &str) -> u64 {
    let bytes_per_second = match mime_type {
        "audio/wav" | "audio/x-wav" | "audio/wave" => 176_400,
        _ => 16_000,
    };

    size_bytes / bytes_per_second
}

fn format_duration(duration_secs: u64) -> String {
    if duration_secs < 60 {
        return format!("{duration_secs}s");
    }

    let minutes = duration_secs / 60;
    let seconds = duration_secs % 60;
    format!("{minutes}m {seconds}s")
}

fn encode_base64(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(CHARS[(triple >> 18 & 0x3F) as usize] as char);
        out.push(CHARS[(triple >> 12 & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[(triple >> 6 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use async_trait::async_trait;

    use crate::core::providers::response::{ImageSource, MessageRole, ProviderResponse};
    use crate::core::tools::traits::ToolSpec;

    #[derive(Debug, Clone, Copy)]
    enum VisionMode {
        Success,
        EmptyText,
        Error,
    }

    struct MockVisionProvider {
        supports_vision: bool,
        mode: VisionMode,
        calls: std::sync::Mutex<Vec<(Option<String>, String, f64, usize)>>,
    }

    impl MockVisionProvider {
        fn new(supports_vision: bool, mode: VisionMode) -> Self {
            Self {
                supports_vision,
                mode,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        fn first_call(&self) -> (Option<String>, String, f64, usize) {
            self.calls.lock().unwrap()[0].clone()
        }
    }

    #[async_trait]
    impl Provider for MockVisionProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok("unused".to_string())
        }

        async fn chat_with_tools(
            &self,
            system_prompt: Option<&str>,
            messages: &[ProviderMessage],
            tools: &[ToolSpec],
            model: &str,
            temperature: f64,
        ) -> anyhow::Result<ProviderResponse> {
            self.calls.lock().unwrap().push((
                system_prompt.map(ToString::to_string),
                model.to_string(),
                temperature,
                tools.len(),
            ));

            assert_eq!(messages.len(), 1);
            assert!(matches!(messages[0].role, MessageRole::User));
            assert!(matches!(&messages[0].content[0], ContentBlock::Text { .. }));
            assert!(matches!(
                &messages[0].content[1],
                ContentBlock::Image { .. }
            ));

            if let ContentBlock::Image { source } = &messages[0].content[1] {
                match source {
                    ImageSource::Base64 { media_type, data } => {
                        assert_eq!(media_type, "image/png");
                        assert_eq!(data, "AQID");
                    }
                    ImageSource::Url { .. } => panic!("expected base64 image source"),
                }
            }

            match self.mode {
                VisionMode::Success => Ok(ProviderResponse::text_only(
                    "A small test image with three bytes.".to_string(),
                )),
                VisionMode::EmptyText => Ok(ProviderResponse {
                    text: "   ".to_string(),
                    input_tokens: None,
                    output_tokens: None,
                    model: None,
                    content_blocks: vec![],
                    stop_reason: None,
                }),
                VisionMode::Error => Err(anyhow!("vision provider failed")),
            }
        }

        fn supports_vision(&self) -> bool {
            self.supports_vision
        }
    }

    fn test_media_file(media_type: MediaType, mime_type: &str, size_bytes: u64) -> MediaFile {
        MediaFile {
            id: "id-1".to_string(),
            mime_type: mime_type.to_string(),
            media_type,
            filename: Some("asset.bin".to_string()),
            size_bytes,
            storage_path: "media/asset.bin".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn describe_image_without_provider_uses_metadata() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Image, "image/png", 3);

        let description = processor.describe(&file, &[1, 2, 3]).await.unwrap();

        assert_eq!(description, "[Image: asset.bin (image/png, 3 bytes)]");
    }

    #[tokio::test]
    async fn describe_audio_without_provider_uses_metadata_with_duration() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Audio, "audio/mpeg", 16_000);

        let description = processor.describe(&file, &[]).await.unwrap();

        assert_eq!(
            description,
            "[Audio: asset.bin (audio/mpeg, 16000 bytes, ~1s estimated)]"
        );
    }

    #[tokio::test]
    async fn describe_image_with_provider_uses_vision_output() {
        let provider = Arc::new(MockVisionProvider::new(true, VisionMode::Success));
        let provider_for_assert = Arc::clone(&provider);
        let processor = MediaProcessor::with_provider(provider, "vision-model".to_string());
        let file = test_media_file(MediaType::Image, "image/png", 3);

        let description = processor.describe(&file, &[1, 2, 3]).await.unwrap();

        assert_eq!(description, "A small test image with three bytes.");
        assert_eq!(provider_for_assert.call_count(), 1);
        assert_eq!(
            provider_for_assert.first_call(),
            (
                Some(IMAGE_DESCRIPTION_PROMPT.to_string()),
                "vision-model".to_string(),
                0.2,
                0,
            )
        );
    }

    #[tokio::test]
    async fn describe_image_skips_provider_when_vision_not_supported() {
        let provider = Arc::new(MockVisionProvider::new(false, VisionMode::Success));
        let provider_for_assert = Arc::clone(&provider);
        let processor = MediaProcessor::with_provider(provider, "vision-model".to_string());
        let file = test_media_file(MediaType::Image, "image/png", 3);

        let description = processor.describe(&file, &[1, 2, 3]).await.unwrap();

        assert_eq!(description, "[Image: asset.bin (image/png, 3 bytes)]");
        assert_eq!(provider_for_assert.call_count(), 0);
    }

    #[tokio::test]
    async fn describe_image_falls_back_when_provider_errors() {
        let provider = Arc::new(MockVisionProvider::new(true, VisionMode::Error));
        let processor = MediaProcessor::with_provider(provider, "vision-model".to_string());
        let file = test_media_file(MediaType::Image, "image/png", 3);

        let description = processor.describe(&file, &[1, 2, 3]).await.unwrap();

        assert_eq!(description, "[Image: asset.bin (image/png, 3 bytes)]");
    }

    #[tokio::test]
    async fn describe_image_falls_back_when_provider_returns_empty_text() {
        let provider = Arc::new(MockVisionProvider::new(true, VisionMode::EmptyText));
        let processor = MediaProcessor::with_provider(provider, "vision-model".to_string());
        let file = test_media_file(MediaType::Image, "image/png", 3);

        let description = processor.describe(&file, &[1, 2, 3]).await.unwrap();

        assert_eq!(description, "[Image: asset.bin (image/png, 3 bytes)]");
    }

    #[tokio::test]
    async fn describe_document_text_includes_preview() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Document, "text/plain", 11);

        let description = processor.describe(&file, b"hello world").await.unwrap();

        assert_eq!(
            description,
            "[Document: asset.bin (text/plain, 11 bytes)] Preview: hello world"
        );
    }

    #[tokio::test]
    async fn describe_document_text_truncates_preview_to_500_chars() {
        let processor = MediaProcessor::new();
        let long_text = "a".repeat(600);
        let file = test_media_file(MediaType::Document, "text/plain", 600);

        let description = processor
            .describe(&file, long_text.as_bytes())
            .await
            .unwrap();

        assert!(description.ends_with(&"a".repeat(500)));
        assert!(!description.ends_with(&"a".repeat(501)));
    }

    #[tokio::test]
    async fn describe_document_pdf_returns_pdf_metadata() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Document, "application/pdf", 42);

        let description = processor.describe(&file, b"%PDF-1.7").await.unwrap();

        assert_eq!(description, "[PDF document: asset.bin (42 bytes)]");
    }

    #[tokio::test]
    async fn describe_document_other_type_returns_generic_metadata() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Document, "application/msword", 84);

        let description = processor.describe(&file, &[0, 1, 2]).await.unwrap();

        assert_eq!(
            description,
            "[Document: asset.bin (application/msword, 84 bytes)]"
        );
    }

    #[tokio::test]
    async fn describe_document_invalid_utf8_falls_back_to_metadata() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Document, "text/plain", 3);

        let description = processor
            .describe(&file, &[0xFF, 0xFE, 0xFD])
            .await
            .unwrap();

        assert_eq!(description, "[Document: asset.bin (text/plain, 3 bytes)]");
    }

    #[tokio::test]
    async fn describe_unknown_type_still_reports_unknown() {
        let processor = MediaProcessor::new();
        let file = test_media_file(MediaType::Unknown, "application/octet-stream", 8);

        let description = processor.describe(&file, &[0; 8]).await.unwrap();

        assert_eq!(description, "[Unknown media type]");
    }

    #[test]
    fn estimate_audio_duration_uses_mp3_heuristic() {
        assert_eq!(estimate_audio_duration_secs(32_000, "audio/mpeg"), 2);
    }

    #[test]
    fn estimate_audio_duration_uses_wav_heuristic() {
        assert_eq!(estimate_audio_duration_secs(352_800, "audio/wav"), 2);
    }

    #[test]
    fn estimate_audio_duration_uses_ogg_heuristic() {
        assert_eq!(estimate_audio_duration_secs(48_000, "audio/ogg"), 3);
    }

    #[test]
    fn estimate_audio_duration_uses_default_heuristic() {
        assert_eq!(estimate_audio_duration_secs(16_000, "audio/flac"), 1);
    }

    #[test]
    fn format_duration_seconds_only() {
        assert_eq!(format_duration(45), "45s");
    }

    #[test]
    fn format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(150), "2m 30s");
    }

    #[test]
    fn encode_base64_matches_expected_output() {
        assert_eq!(encode_base64(&[1, 2, 3]), "AQID");
    }
}
