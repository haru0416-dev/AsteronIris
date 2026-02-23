use crate::core::providers::response::ContentBlock;
use crate::core::tools::OutputAttachment;
use crate::media::{MediaProcessor, MediaStore};
use anyhow::Result;
use std::sync::Arc;

use super::traits::{MediaAttachment, MediaData};

pub(crate) fn media_attachment_url(
    url: String,
    mime_type: Option<&str>,
    filename: Option<String>,
) -> MediaAttachment {
    MediaAttachment {
        mime_type: mime_type.unwrap_or("application/octet-stream").to_string(),
        data: MediaData::Url(url),
        filename,
    }
}

pub(crate) fn convert_attachments_to_images(attachments: &[MediaAttachment]) -> Vec<ContentBlock> {
    use crate::core::providers::response::ImageSource;

    attachments
        .iter()
        .filter(|a| a.mime_type.starts_with("image/"))
        .map(|a| {
            let source = match &a.data {
                MediaData::Url(url) => ImageSource::url(url),
                MediaData::Bytes(bytes) => ImageSource::base64(&a.mime_type, encode_base64(bytes)),
            };
            ContentBlock::Image { source }
        })
        .collect()
}

pub(crate) fn encode_base64(bytes: &[u8]) -> String {
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

pub(crate) fn attachment_to_image_block(attachment: &MediaAttachment) -> Option<ContentBlock> {
    use crate::core::providers::response::ImageSource;

    if !attachment.mime_type.starts_with("image/") {
        return None;
    }

    let source = match &attachment.data {
        MediaData::Url(url) => ImageSource::url(url),
        MediaData::Bytes(bytes) => ImageSource::base64(&attachment.mime_type, encode_base64(bytes)),
    };
    Some(ContentBlock::Image { source })
}

pub(crate) async fn load_attachment_bytes(attachment: &MediaAttachment) -> Result<Vec<u8>> {
    match &attachment.data {
        MediaData::Bytes(bytes) => Ok(bytes.clone()),
        MediaData::Url(url) => {
            let response = reqwest::get(url).await?;
            let response = response.error_for_status()?;
            let bytes = response.bytes().await?;
            Ok(bytes.to_vec())
        }
    }
}

pub(crate) fn fallback_attachment_description(
    attachment: &MediaAttachment,
    size_bytes: Option<usize>,
) -> String {
    let filename = attachment.filename.as_deref().unwrap_or("unnamed");
    let size_part = size_bytes
        .map(|bytes| format!(", {}KB", bytes.div_ceil(1024)))
        .unwrap_or_default();
    format!(
        "[Attachment: {filename} ({}{size_part})]",
        attachment.mime_type
    )
}

pub(crate) async fn prepare_channel_input_and_images(
    model_input: &str,
    attachments: &[MediaAttachment],
    media_store: Option<&Arc<MediaStore>>,
    processor: &MediaProcessor,
) -> (String, Vec<ContentBlock>) {
    let Some(store) = media_store else {
        return (
            model_input.to_string(),
            convert_attachments_to_images(attachments),
        );
    };
    let mut attachment_descriptions = Vec::new();
    let mut image_blocks = Vec::new();

    for attachment in attachments {
        match load_attachment_bytes(attachment).await {
            Ok(bytes) => match store.store(&bytes, attachment.filename.as_deref()) {
                Ok(stored) => {
                    if !attachment.mime_type.starts_with("image/") {
                        match processor.describe(&stored, &bytes).await {
                            Ok(description) => attachment_descriptions.push(description),
                            Err(error) => {
                                tracing::warn!(
                                    channel_attachment = ?attachment.filename,
                                    error = %error,
                                    "failed to describe non-image attachment"
                                );
                                attachment_descriptions.push(fallback_attachment_description(
                                    attachment,
                                    Some(bytes.len()),
                                ));
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        channel_attachment = ?attachment.filename,
                        error = %error,
                        "failed to persist attachment"
                    );
                    if !attachment.mime_type.starts_with("image/") {
                        attachment_descriptions.push(fallback_attachment_description(
                            attachment,
                            Some(bytes.len()),
                        ));
                    }
                }
            },
            Err(error) => {
                tracing::warn!(
                    channel_attachment = ?attachment.filename,
                    error = %error,
                    "failed to load attachment bytes"
                );
                if !attachment.mime_type.starts_with("image/") {
                    attachment_descriptions.push(fallback_attachment_description(attachment, None));
                }
            }
        }

        if let Some(block) = attachment_to_image_block(attachment) {
            image_blocks.push(block);
        }
    }

    if attachment_descriptions.is_empty() {
        (model_input.to_string(), image_blocks)
    } else {
        let prefix = attachment_descriptions.join("\n");
        (format!("{prefix}\n\n{model_input}"), image_blocks)
    }
}

pub(crate) async fn output_attachment_to_media_attachment(
    attachment: &OutputAttachment,
) -> Option<MediaAttachment> {
    if let Some(path) = &attachment.path {
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                return Some(MediaAttachment {
                    mime_type: attachment.mime_type.clone(),
                    data: MediaData::Bytes(bytes),
                    filename: attachment.filename.clone(),
                });
            }
            Err(error) => {
                tracing::trace!(
                    path = %path,
                    mime_type = %attachment.mime_type,
                    error = %error,
                    "failed to read output attachment path"
                );
                return None;
            }
        }
    }

    if let Some(url) = &attachment.url {
        return Some(MediaAttachment {
            mime_type: attachment.mime_type.clone(),
            data: MediaData::Url(url.clone()),
            filename: attachment.filename.clone(),
        });
    }

    tracing::trace!(
        mime_type = %attachment.mime_type,
        filename = ?attachment.filename,
        "skipping output attachment without path or url"
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::providers::response::ContentBlock;
    use crate::core::tools::OutputAttachment;
    use crate::media::{MediaProcessor, MediaStore};
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_media_store(temp_dir: &TempDir, max_file_size_mb: u64) -> Arc<MediaStore> {
        let workspace = temp_dir.path().to_string_lossy().into_owned();
        let config = crate::media::types::MediaConfig {
            enabled: true,
            storage_dir: None,
            max_file_size_mb,
        };
        Arc::new(MediaStore::new(&config, &workspace).unwrap())
    }

    fn stored_file_count(temp_dir: &TempDir) -> usize {
        fs::read_dir(temp_dir.path().join("media"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy() != "media.db")
            .count()
    }

    #[test]
    fn encode_base64_empty() {
        assert_eq!(encode_base64(&[]), "");
    }

    #[test]
    fn encode_base64_hello() {
        assert_eq!(encode_base64(b"Hello"), "SGVsbG8=");
    }

    #[test]
    fn encode_base64_three_byte_aligned() {
        assert_eq!(encode_base64(b"abc"), "YWJj");
    }

    #[test]
    fn convert_attachments_filters_non_images() {
        let attachments = vec![
            MediaAttachment {
                mime_type: "image/png".to_string(),
                data: MediaData::Url("https://example.com/img.png".to_string()),
                filename: Some("img.png".to_string()),
            },
            MediaAttachment {
                mime_type: "audio/mpeg".to_string(),
                data: MediaData::Url("https://example.com/audio.mp3".to_string()),
                filename: Some("audio.mp3".to_string()),
            },
        ];
        let blocks = convert_attachments_to_images(&attachments);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], ContentBlock::Image { .. }));
    }

    #[test]
    fn convert_attachments_url_source() {
        let attachments = vec![MediaAttachment {
            mime_type: "image/jpeg".to_string(),
            data: MediaData::Url("https://example.com/photo.jpg".to_string()),
            filename: None,
        }];
        let blocks = convert_attachments_to_images(&attachments);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Image { source } = &blocks[0] {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "url");
            assert_eq!(json["url"], "https://example.com/photo.jpg");
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn convert_attachments_bytes_source() {
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            filename: Some("test.png".to_string()),
        }];
        let blocks = convert_attachments_to_images(&attachments);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::Image { source } = &blocks[0] {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "base64");
            assert_eq!(json["media_type"], "image/png");
            assert!(!json["data"].as_str().unwrap().is_empty());
        } else {
            panic!("expected Image block");
        }
    }

    #[test]
    fn convert_attachments_empty() {
        let blocks = convert_attachments_to_images(&[]);
        assert!(blocks.is_empty());
    }

    #[test]
    fn attachment_to_image_block_returns_none_for_non_images() {
        let attachment = MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Bytes(vec![1, 2, 3]),
            filename: Some("clip.mp3".to_string()),
        };

        assert!(attachment_to_image_block(&attachment).is_none());
    }

    #[test]
    fn attachment_to_image_block_returns_url_variant() {
        let attachment = MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Url("https://example.com/a.png".to_string()),
            filename: Some("a.png".to_string()),
        };

        let block = attachment_to_image_block(&attachment).unwrap();
        if let ContentBlock::Image { source } = block {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "url");
            assert_eq!(json["url"], "https://example.com/a.png");
        } else {
            panic!("expected image block");
        }
    }

    #[test]
    fn fallback_attachment_description_includes_size_when_known() {
        let attachment = MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Bytes(vec![0_u8; 2048]),
            filename: Some("doc.pdf".to_string()),
        };

        let description = fallback_attachment_description(&attachment, Some(2048));
        assert_eq!(description, "[Attachment: doc.pdf (application/pdf, 2KB)]");
    }

    #[test]
    fn fallback_attachment_description_omits_size_when_unknown() {
        let attachment = MediaAttachment {
            mime_type: "application/octet-stream".to_string(),
            data: MediaData::Url("https://example.com/blob".to_string()),
            filename: None,
        };

        let description = fallback_attachment_description(&attachment, None);
        assert_eq!(
            description,
            "[Attachment: unnamed (application/octet-stream)]"
        );
    }

    #[tokio::test]
    async fn load_attachment_bytes_returns_raw_bytes_variant() {
        let attachment = MediaAttachment {
            mime_type: "text/plain".to_string(),
            data: MediaData::Bytes(vec![7, 8, 9]),
            filename: Some("note.txt".to_string()),
        };

        let loaded = load_attachment_bytes(&attachment).await.unwrap();
        assert_eq!(loaded, vec![7, 8, 9]);
    }

    #[tokio::test]
    async fn load_attachment_bytes_downloads_url_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/file.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 3, 3, 7]))
            .mount(&server)
            .await;

        let attachment = MediaAttachment {
            mime_type: "application/octet-stream".to_string(),
            data: MediaData::Url(format!("{}/file.bin", server.uri())),
            filename: Some("file.bin".to_string()),
        };

        let loaded = load_attachment_bytes(&attachment).await.unwrap();
        assert_eq!(loaded, vec![1, 3, 3, 7]);
    }

    #[tokio::test]
    async fn prepare_channel_input_media_disabled_keeps_behavior() {
        let processor = MediaProcessor::new();
        let attachments = vec![
            MediaAttachment {
                mime_type: "image/png".to_string(),
                data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
                filename: Some("inline.png".to_string()),
            },
            MediaAttachment {
                mime_type: "audio/mpeg".to_string(),
                data: MediaData::Bytes(vec![1, 2, 3]),
                filename: Some("sound.mp3".to_string()),
            },
        ];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, None, &processor).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_adds_description_and_stores() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Bytes(vec![1, 2, 3, 4]),
            filename: Some("sound.mp3".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert!(input.starts_with("[Audio: sound.mp3 (audio/mpeg, 4 bytes"));
        assert!(input.ends_with("\n\nhello"));
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_image_bytes_remains_inline_and_is_stored() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            filename: Some("img.png".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_image_url_is_downloaded_stored_and_forwarded_as_url() {
        let processor = MediaProcessor::new();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/img.png"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(vec![0x89, 0x50, 0x4E, 0x47]),
            )
            .mount(&server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "image/png".to_string(),
            data: MediaData::Url(format!("{}/img.png", server.uri())),
            filename: Some("img.png".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(input, "hello");
        assert_eq!(images.len(), 1);
        if let ContentBlock::Image { source } = &images[0] {
            let json = serde_json::to_value(source).unwrap();
            assert_eq!(json["type"], "url");
        } else {
            panic!("expected image block");
        }
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_url_downloads_and_describes() {
        let processor = MediaProcessor::new();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/voice.mp3"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "audio/mpeg")
                    .set_body_bytes(vec![0x49, 0x44, 0x33, 0x00]),
            )
            .mount(&server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![MediaAttachment {
            mime_type: "audio/mpeg".to_string(),
            data: MediaData::Url(format!("{}/voice.mp3", server.uri())),
            filename: Some("voice.mp3".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert!(input.contains("[Audio: voice.mp3 (audio/mpeg, 4 bytes"));
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 1);
    }

    #[tokio::test]
    async fn prepare_channel_input_non_image_url_download_failure_falls_back() {
        let processor = MediaProcessor::new();
        let attachments = vec![MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Url("http://127.0.0.1:9/missing.pdf".to_string()),
            filename: Some("missing.pdf".to_string()),
        }];

        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(
            input,
            "[Attachment: missing.pdf (application/pdf)]\n\nhello"
        );
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 0);
    }

    #[tokio::test]
    async fn prepare_channel_input_store_failure_falls_back_for_non_image() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 0);
        let attachments = vec![MediaAttachment {
            mime_type: "application/pdf".to_string(),
            data: MediaData::Bytes(vec![1]),
            filename: Some("doc.pdf".to_string()),
        }];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert_eq!(
            input,
            "[Attachment: doc.pdf (application/pdf, 1KB)]\n\nhello"
        );
        assert!(images.is_empty());
        assert_eq!(stored_file_count(&temp_dir), 0);
    }

    #[tokio::test]
    async fn prepare_channel_input_mixed_attachments_preserves_images_and_adds_text_prefix() {
        let processor = MediaProcessor::new();
        let temp_dir = TempDir::new().unwrap();
        let store = test_media_store(&temp_dir, 25);
        let attachments = vec![
            MediaAttachment {
                mime_type: "audio/mpeg".to_string(),
                data: MediaData::Bytes(vec![1, 2, 3]),
                filename: Some("clip.mp3".to_string()),
            },
            MediaAttachment {
                mime_type: "image/png".to_string(),
                data: MediaData::Bytes(vec![0x89, 0x50, 0x4E, 0x47]),
                filename: Some("img.png".to_string()),
            },
        ];

        let (input, images) =
            prepare_channel_input_and_images("hello", &attachments, Some(&store), &processor).await;

        assert!(input.starts_with("[Audio: clip.mp3 (audio/mpeg, 3 bytes"));
        assert!(input.ends_with("\n\nhello"));
        assert_eq!(images.len(), 1);
        assert_eq!(stored_file_count(&temp_dir), 2);
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_reads_bytes_from_path() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("generated.bin");
        fs::write(&path, [1_u8, 2, 3, 4]).unwrap();
        let attachment = OutputAttachment::from_path(
            "application/octet-stream",
            path.to_string_lossy().to_string(),
            Some("generated.bin".to_string()),
        );

        let media = output_attachment_to_media_attachment(&attachment)
            .await
            .unwrap();
        match media.data {
            MediaData::Bytes(bytes) => assert_eq!(bytes, vec![1, 2, 3, 4]),
            MediaData::Url(_) => panic!("expected bytes media data"),
        }
        assert_eq!(media.mime_type, "application/octet-stream");
        assert_eq!(media.filename.as_deref(), Some("generated.bin"));
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_maps_url_variant() {
        let attachment = OutputAttachment::from_url(
            "image/png",
            "https://example.com/a.png",
            Some("a.png".to_string()),
        );

        let media = output_attachment_to_media_attachment(&attachment)
            .await
            .unwrap();
        match media.data {
            MediaData::Url(url) => assert_eq!(url, "https://example.com/a.png"),
            MediaData::Bytes(_) => panic!("expected url media data"),
        }
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_missing_path_returns_none() {
        let attachment = OutputAttachment::from_path(
            "image/png",
            "/tmp/does-not-exist.png",
            Some("missing.png".to_string()),
        );

        let media = output_attachment_to_media_attachment(&attachment).await;
        assert!(media.is_none());
    }

    #[tokio::test]
    async fn output_attachment_to_media_attachment_without_location_returns_none() {
        let attachment = OutputAttachment {
            mime_type: "image/png".to_string(),
            filename: Some("img.png".to_string()),
            path: None,
            url: None,
        };

        let media = output_attachment_to_media_attachment(&attachment).await;
        assert!(media.is_none());
    }
}
