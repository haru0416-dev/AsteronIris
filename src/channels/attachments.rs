use crate::intelligence::providers::response::ContentBlock;
use crate::intelligence::tools::OutputAttachment;
use crate::media::{MediaProcessor, MediaStore};
use anyhow::Result;
use std::sync::Arc;

use super::traits::{MediaAttachment, MediaData};

pub(crate) fn convert_attachments_to_images(attachments: &[MediaAttachment]) -> Vec<ContentBlock> {
    use crate::intelligence::providers::response::ImageSource;

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
    use crate::intelligence::providers::response::ImageSource;

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
