use super::types::{MediaConfig, MediaFile, MediaType};
use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct MediaStore {
    storage_dir: PathBuf,
    conn: Arc<Mutex<Connection>>,
    max_file_size: u64,
}

impl MediaStore {
    pub async fn new(config: &MediaConfig, workspace_dir: &Path) -> Result<Self> {
        let storage_dir = config
            .storage_dir
            .as_deref()
            .map_or_else(|| workspace_dir.join("media"), PathBuf::from);

        tokio::fs::create_dir_all(&storage_dir).await?;

        let db_path = storage_dir.join("media.db");
        let conn = tokio::task::spawn_blocking(move || -> Result<Connection> {
            let conn = Connection::open(&db_path)?;
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
            Ok(conn)
        })
        .await??;

        Ok(Self {
            storage_dir,
            conn: Arc::new(Mutex::new(conn)),
            max_file_size: config.max_file_size_mb * 1_024 * 1_024,
        })
    }

    pub async fn store(&self, data: &[u8], filename: Option<&str>) -> Result<MediaFile> {
        let size = data.len() as u64;
        if size > self.max_file_size {
            anyhow::bail!(
                "file size {} exceeds maximum {} bytes",
                size,
                self.max_file_size
            );
        }

        let id = uuid::Uuid::new_v4().to_string();
        let (mime_type, media_type) = super::detection::detect_media_type(data, filename);

        let ext = extension_from_mime(&mime_type);
        let storage_filename = format!("{id}.{ext}");
        let storage_path = self.storage_dir.join(&storage_filename);

        tokio::fs::write(&storage_path, data).await?;

        let created_at = chrono::Utc::now().to_rfc3339();
        let storage_path_str = storage_path.to_string_lossy().into_owned();

        let media_file = MediaFile {
            id,
            mime_type,
            media_type,
            filename: filename.map(String::from),
            size_bytes: size,
            storage_path: storage_path_str,
            created_at,
        };

        let file_clone = media_file.clone();
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = conn.lock().map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
            let size_i64 = i64::try_from(file_clone.size_bytes)?;
            conn.execute(
                "INSERT INTO media_files (id, mime_type, media_type, filename, size_bytes, storage_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    file_clone.id,
                    file_clone.mime_type,
                    file_clone.media_type.as_str(),
                    file_clone.filename,
                    size_i64,
                    file_clone.storage_path,
                    file_clone.created_at,
                ],
            )?;
            Ok(())
        })
        .await??;

        Ok(media_file)
    }

    pub async fn retrieve(&self, id: &str) -> Result<(MediaFile, Vec<u8>)> {
        let conn = Arc::clone(&self.conn);
        let id_owned = id.to_string();
        let media_file = tokio::task::spawn_blocking(move || -> Result<MediaFile> {
            let conn = conn
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
            let mut stmt = conn.prepare(
                "SELECT id, mime_type, media_type, filename, size_bytes, storage_path, created_at
                 FROM media_files
                 WHERE id = ?1",
            )?;
            let file = stmt.query_row([&id_owned], |row| {
                let media_type_str: String = row.get(2)?;
                let size_i64: i64 = row.get(4)?;
                let size_bytes = u64::try_from(size_i64).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Integer,
                        Box::new(e),
                    )
                })?;
                Ok(MediaFile {
                    id: row.get(0)?,
                    mime_type: row.get(1)?,
                    media_type: MediaType::from_kind(&media_type_str),
                    filename: row.get(3)?,
                    size_bytes,
                    storage_path: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;
            Ok(file)
        })
        .await??;

        let data = tokio::fs::read(&media_file.storage_path).await?;
        Ok((media_file, data))
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let id_owned = id.to_string();
        let storage_path = tokio::task::spawn_blocking(move || -> Result<String> {
            let conn = conn
                .lock()
                .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
            let path: String = conn.query_row(
                "SELECT storage_path FROM media_files WHERE id = ?1",
                [&id_owned],
                |row| row.get(0),
            )?;
            conn.execute("DELETE FROM media_files WHERE id = ?1", [&id_owned])?;
            Ok(path)
        })
        .await??;

        let path = Path::new(&storage_path);
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }

        Ok(())
    }

    #[must_use]
    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
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

    #[tokio::test]
    async fn store_and_retrieve_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let store = MediaStore::new(&MediaConfig::default(), temp_dir.path())
            .await
            .unwrap();

        let data = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        let stored = store.store(&data, Some("sample.png")).await.unwrap();
        let (retrieved, bytes) = store.retrieve(&stored.id).await.unwrap();

        assert_eq!(bytes, data);
        assert_eq!(retrieved.id, stored.id);
        assert_eq!(retrieved.mime_type, "image/png");
        assert_eq!(retrieved.filename.as_deref(), Some("sample.png"));
    }

    #[tokio::test]
    async fn store_rejects_oversized_file() {
        let temp_dir = TempDir::new().unwrap();
        let config = MediaConfig {
            max_file_size_mb: 1,
            ..MediaConfig::default()
        };
        let store = MediaStore::new(&config, temp_dir.path()).await.unwrap();

        let oversized = vec![0_u8; (1_024 * 1_024) + 1];
        let result = store.store(&oversized, Some("too_large.bin")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn store_creates_file_on_disk() {
        let temp_dir = TempDir::new().unwrap();
        let store = MediaStore::new(&MediaConfig::default(), temp_dir.path())
            .await
            .unwrap();

        let data = b"hello";
        let stored = store.store(data, Some("hello.txt")).await.unwrap();
        assert!(Path::new(&stored.storage_path).exists());
    }

    #[tokio::test]
    async fn retrieve_errors_for_nonexistent_id() {
        let temp_dir = TempDir::new().unwrap();
        let store = MediaStore::new(&MediaConfig::default(), temp_dir.path())
            .await
            .unwrap();

        let result = store.retrieve("missing-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_removes_file_and_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let store = MediaStore::new(&MediaConfig::default(), temp_dir.path())
            .await
            .unwrap();

        let data = b"to be deleted";
        let stored = store.store(data, Some("delete_me.txt")).await.unwrap();
        let id = stored.id.clone();
        let path = stored.storage_path.clone();

        assert!(Path::new(&path).exists());

        store.delete(&id).await.unwrap();

        assert!(!Path::new(&path).exists());
        assert!(store.retrieve(&id).await.is_err());
    }

    #[tokio::test]
    async fn delete_errors_for_nonexistent_id() {
        let temp_dir = TempDir::new().unwrap();
        let store = MediaStore::new(&MediaConfig::default(), temp_dir.path())
            .await
            .unwrap();

        let result = store.delete("nonexistent-id").await;
        assert!(result.is_err());
    }
}
