use super::types::MediaType;

#[must_use]
pub fn detect_mime(data: &[u8]) -> Option<String> {
    infer::get(data).map(|info| info.mime_type().to_string())
}

#[must_use]
pub fn detect_mime_from_extension(filename: &str) -> Option<String> {
    let ext = filename.rsplit('.').next()?;
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "png" => Some("image/png".into()),
        "gif" => Some("image/gif".into()),
        "webp" => Some("image/webp".into()),
        "mp3" => Some("audio/mpeg".into()),
        "wav" => Some("audio/wav".into()),
        "ogg" => Some("audio/ogg".into()),
        "mp4" => Some("video/mp4".into()),
        "webm" => Some("video/webm".into()),
        "pdf" => Some("application/pdf".into()),
        _ => None,
    }
}

#[must_use]
pub fn detect_media_type(data: &[u8], filename: Option<&str>) -> (String, MediaType) {
    let mime = detect_mime(data)
        .or_else(|| filename.and_then(detect_mime_from_extension))
        .unwrap_or_else(|| "application/octet-stream".into());
    let media_type = MediaType::from_mime(&mime);
    (mime, media_type)
}

#[cfg(test)]
mod tests {
    use super::{detect_media_type, detect_mime, detect_mime_from_extension};
    use crate::media::types::MediaType;

    #[test]
    fn detect_mime_png_magic_bytes() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        assert_eq!(detect_mime(&png).as_deref(), Some("image/png"));
    }

    #[test]
    fn detect_mime_jpeg_magic_bytes() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        assert_eq!(detect_mime(&jpeg).as_deref(), Some("image/jpeg"));
    }

    #[test]
    fn detect_mime_unknown_returns_none() {
        let unknown = [0x00, 0x11, 0x22, 0x33, 0x44];
        assert!(detect_mime(&unknown).is_none());
    }

    #[test]
    fn detect_mime_from_extension_common_types() {
        assert_eq!(
            detect_mime_from_extension("photo.JPG").as_deref(),
            Some("image/jpeg")
        );
        assert_eq!(
            detect_mime_from_extension("clip.webm").as_deref(),
            Some("video/webm")
        );
        assert_eq!(
            detect_mime_from_extension("voice.mp3").as_deref(),
            Some("audio/mpeg")
        );
        assert_eq!(
            detect_mime_from_extension("report.pdf").as_deref(),
            Some("application/pdf")
        );
    }

    #[test]
    fn detect_media_type_combines_magic_and_extension_fallback() {
        let unknown = [0x00, 0x11, 0x22, 0x33];
        let (mime, media_type) = detect_media_type(&unknown, Some("image.png"));
        assert_eq!(mime, "image/png");
        assert_eq!(media_type, MediaType::Image);

        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        let (mime_from_bytes, media_type_from_bytes) = detect_media_type(&png, Some("file.bin"));
        assert_eq!(mime_from_bytes, "image/png");
        assert_eq!(media_type_from_bytes, MediaType::Image);
    }

    #[test]
    fn media_type_from_mime_covers_categories() {
        assert_eq!(MediaType::from_mime("image/webp"), MediaType::Image);
        assert_eq!(MediaType::from_mime("audio/wav"), MediaType::Audio);
        assert_eq!(MediaType::from_mime("video/mp4"), MediaType::Video);
        assert_eq!(MediaType::from_mime("text/markdown"), MediaType::Document);
        assert_eq!(
            MediaType::from_mime("application/octet-stream"),
            MediaType::Unknown
        );
    }
}
