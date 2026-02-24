use crate::llm::types::ContentBlock;
use crate::tools::OutputAttachment;
use anyhow::Result;

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
    use crate::llm::types::ImageSource;

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
    use crate::llm::types::ImageSource;

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
    use crate::llm::types::ContentBlock;
    use crate::tools::OutputAttachment;

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
