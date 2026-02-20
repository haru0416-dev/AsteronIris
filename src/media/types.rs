use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Image,
    Audio,
    Video,
    Document,
    Unknown,
}

impl MediaType {
    #[must_use]
    pub fn from_mime(mime: &str) -> Self {
        if mime.starts_with("image/") {
            Self::Image
        } else if mime.starts_with("audio/") {
            Self::Audio
        } else if mime.starts_with("video/") {
            Self::Video
        } else if mime.starts_with("application/pdf") || mime.starts_with("text/") {
            Self::Document
        } else {
            Self::Unknown
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Document => "document",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "image" => Self::Image,
            "audio" => Self::Audio,
            "video" => Self::Video,
            "document" => Self::Document,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub id: String,
    pub mime_type: String,
    pub media_type: MediaType,
    pub filename: Option<String>,
    pub size_bytes: u64,
    pub storage_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMedia {
    pub file: MediaFile,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConfig {
    pub enabled: bool,
    pub storage_dir: Option<String>,
    pub max_file_size_mb: u64,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            storage_dir: None,
            max_file_size_mb: 25,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaConfig, MediaType};

    #[test]
    fn media_config_defaults_match_expected_values() {
        let config = MediaConfig::default();
        assert!(!config.enabled);
        assert!(config.storage_dir.is_none());
        assert_eq!(config.max_file_size_mb, 25);
    }

    #[test]
    fn media_type_from_mime_maps_all_variants() {
        assert_eq!(MediaType::from_mime("image/png"), MediaType::Image);
        assert_eq!(MediaType::from_mime("audio/mpeg"), MediaType::Audio);
        assert_eq!(MediaType::from_mime("video/mp4"), MediaType::Video);
        assert_eq!(MediaType::from_mime("application/pdf"), MediaType::Document);
        assert_eq!(MediaType::from_mime("text/plain"), MediaType::Document);
        assert_eq!(
            MediaType::from_mime("application/octet-stream"),
            MediaType::Unknown
        );
    }
}
