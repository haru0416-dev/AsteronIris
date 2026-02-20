use super::types::{MediaConfig, MediaFile, MediaType};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};

const METADATA_DB_FILE: &str = "media.db";

pub struct MediaStore {
    storage_dir: PathBuf,
    metadata_db_path: PathBuf,
    max_file_size: u64,
}

impl MediaStore {
    pub fn new(config: &MediaConfig, workspace_dir: &str) -> Result<Self> {
        let storage_dir = config
            .storage_dir
            .as_deref()
            .map_or_else(|| PathBuf::from(workspace_dir).join("media"), PathBuf::from);

        std::fs::create_dir_all(&storage_dir)?;

        let metadata_db_path = storage_dir.join(METADATA_DB_FILE);
        let store = Self {
            storage_dir,
            metadata_db_path,
            max_file_size: config.max_file_size_mb * 1_024 * 1_024,
        };
        store.init_db()?;

        Ok(store)
    }

    pub fn store(&self, data: &[u8], filename: Option<&str>) -> Result<MediaFile> {
        if data.len() as u64 > self.max_file_size {
            anyhow::bail!(
                "file size {} exceeds maximum {} bytes",
                data.len(),
                self.max_file_size
            );
        }

        let id = uuid::Uuid::new_v4().to_string();
        let (mime_type, media_type) = super::detection::detect_media_type(data, filename);

        let ext = extension_from_mime(&mime_type);
        let storage_filename = format!("{id}.{ext}");
        let storage_path = self.storage_dir.join(storage_filename);

        std::fs::write(&storage_path, data)?;

        let created_at = chrono::Utc::now().to_rfc3339();
        let media_file = MediaFile {
            id,
            mime_type,
            media_type,
            filename: filename.map(String::from),
            size_bytes: data.len() as u64,
            storage_path: storage_path.to_string_lossy().to_string(),
            created_at,
        };
        self.persist_metadata(&media_file)?;

        Ok(media_file)
    }

    pub fn retrieve(&self, id: &str) -> Result<(MediaFile, Vec<u8>)> {
        let media_file = self.load_metadata(id)?;
        let data = std::fs::read(&media_file.storage_path)?;
        Ok((media_file, data))
    }

    #[must_use]
    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
    }

    fn init_db(&self) -> Result<()> {
        let conn = Connection::open(&self.metadata_db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS media_files (
                id TEXT PRIMARY KEY,
                mime_type TEXT NOT NULL,
                media_type TEXT NOT NULL,
                filename TEXT,
                size_bytes INTEGER NOT NULL,
                storage_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    fn persist_metadata(&self, media_file: &MediaFile) -> Result<()> {
        let conn = Connection::open(&self.metadata_db_path)?;
        let size_bytes = i64::try_from(media_file.size_bytes)?;
        conn.execute(
            "INSERT INTO media_files (
                id, mime_type, media_type, filename, size_bytes, storage_path, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                media_file.id,
                media_file.mime_type,
                media_file.media_type.as_str(),
                media_file.filename,
                size_bytes,
                media_file.storage_path,
                media_file.created_at,
            ],
        )?;
        Ok(())
    }

    fn load_metadata(&self, id: &str) -> Result<MediaFile> {
        let conn = Connection::open(&self.metadata_db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, mime_type, media_type, filename, size_bytes, storage_path, created_at
             FROM media_files
             WHERE id = ?1",
        )?;
        let media_file = stmt.query_row([id], |row| {
            let media_type: String = row.get(2)?;
            let size_bytes_i64: i64 = row.get(4)?;
            let size_bytes = u64::try_from(size_bytes_i64).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    4,
                    rusqlite::types::Type::Integer,
                    Box::new(e),
                )
            })?;
            Ok(MediaFile {
                id: row.get(0)?,
                mime_type: row.get(1)?,
                media_type: MediaType::from_kind(&media_type),
                filename: row.get(3)?,
                size_bytes,
                storage_path: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(media_file)
    }
}

fn extension_from_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::MediaStore;
    use crate::media::types::MediaConfig;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn store_and_retrieve_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_dir = temp_dir.path().to_string_lossy().to_string();
        let store = MediaStore::new(&MediaConfig::default(), &workspace_dir).unwrap();

        let data = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        let stored = store.store(&data, Some("sample.png")).unwrap();
        let (retrieved, bytes) = store.retrieve(&stored.id).unwrap();

        assert_eq!(bytes, data);
        assert_eq!(retrieved.id, stored.id);
        assert_eq!(retrieved.mime_type, "image/png");
        assert_eq!(retrieved.filename.as_deref(), Some("sample.png"));
    }

    #[test]
    fn store_rejects_oversized_file() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_dir = temp_dir.path().to_string_lossy().to_string();
        let config = MediaConfig {
            max_file_size_mb: 1,
            ..MediaConfig::default()
        };
        let store = MediaStore::new(&config, &workspace_dir).unwrap();

        let oversized = vec![0_u8; (1_024 * 1_024) + 1];
        let result = store.store(&oversized, Some("too_large.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn store_creates_file_on_disk() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_dir = temp_dir.path().to_string_lossy().to_string();
        let store = MediaStore::new(&MediaConfig::default(), &workspace_dir).unwrap();

        let data = b"hello";
        let stored = store.store(data, Some("hello.txt")).unwrap();
        assert!(Path::new(&stored.storage_path).exists());
    }

    #[test]
    fn retrieve_errors_for_nonexistent_id() {
        let temp_dir = TempDir::new().unwrap();
        let workspace_dir = temp_dir.path().to_string_lossy().to_string();
        let store = MediaStore::new(&MediaConfig::default(), &workspace_dir).unwrap();

        let result = store.retrieve("missing-id");
        assert!(result.is_err());
    }
}
