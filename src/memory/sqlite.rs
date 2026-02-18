use super::embeddings::EmbeddingProvider;
use super::traits::{
    BeliefSlot, ForgetMode, ForgetOutcome, Memory, MemoryCategory, MemoryEvent, MemoryEventInput,
    MemoryLayer, MemoryRecallItem, MemorySource, PrivacyLevel, RecallQuery,
};
use super::vector;
use async_trait::async_trait;
use chrono::Local;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[path = "sqlite/events.rs"]
mod events;
#[path = "sqlite/projection.rs"]
mod projection;

/// SQLite-backed persistent memory — the brain
///
/// Full-stack search engine:
/// - **Vector DB**: embeddings stored as BLOB, cosine similarity search
/// - **Keyword Search**: FTS5 virtual table with BM25 scoring
/// - **Hybrid Merge**: weighted fusion of vector + keyword results
/// - **Embedding Cache**: LRU-evicted cache to avoid redundant API calls
/// - **Safe Reindex**: temp DB → seed → sync → atomic swap → rollback
pub struct SqliteMemory {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    embedder: Arc<dyn EmbeddingProvider>,
    vector_weight: f32,
    keyword_weight: f32,
    cache_max: usize,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
impl SqliteMemory {
    const MEMORY_SCHEMA_V1: i64 = 1;
    const MEMORY_SCHEMA_V2: i64 = 2;
    const MEMORY_SCHEMA_V3: i64 = 3;
    const MEMORY_EVENTS_V2_COLUMNS: [&'static str; 4] = [
        "layer",
        "provenance_source_class",
        "provenance_reference",
        "provenance_evidence_uri",
    ];
    const MEMORY_EVENTS_V3_COLUMNS: [&'static str; 2] = ["retention_tier", "retention_expires_at"];
    const MEMORIES_V3_COLUMNS: [&'static str; 6] = [
        "layer",
        "provenance_source_class",
        "provenance_reference",
        "provenance_evidence_uri",
        "retention_tier",
        "retention_expires_at",
    ];
    const RETRIEVAL_DOCS_V3_COLUMNS: [&'static str; 6] = [
        "layer",
        "provenance_source_class",
        "provenance_reference",
        "provenance_evidence_uri",
        "retention_tier",
        "retention_expires_at",
    ];
    const TREND_TTL_DAYS: f64 = 30.0;
    const TREND_DECAY_WINDOW_DAYS: f64 = 45.0;

    pub fn new(workspace_dir: &Path) -> anyhow::Result<Self> {
        Self::with_embedder(
            workspace_dir,
            Arc::new(super::embeddings::NoopEmbedding),
            0.7,
            0.3,
            10_000,
        )
    }

    pub fn with_embedder(
        workspace_dir: &Path,
        embedder: Arc<dyn EmbeddingProvider>,
        vector_weight: f32,
        keyword_weight: f32,
        cache_max: usize,
    ) -> anyhow::Result<Self> {
        let db_path = workspace_dir.join("memory").join("brain.db");

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;",
        )?;
        Self::init_schema(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
            db_path,
            embedder,
            vector_weight,
            keyword_weight,
            cache_max,
        })
    }

    /// Initialize all tables: memories, FTS5, `embedding_cache`
    #[allow(clippy::too_many_lines)]
    fn init_schema(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "-- Core memories table
            CREATE TABLE IF NOT EXISTS memories (
                id          TEXT PRIMARY KEY,
                key         TEXT NOT NULL UNIQUE,
                content     TEXT NOT NULL,
                category    TEXT NOT NULL DEFAULT 'core',
                layer       TEXT NOT NULL DEFAULT 'working',
                provenance_source_class TEXT,
                provenance_reference TEXT,
                provenance_evidence_uri TEXT,
                retention_tier TEXT NOT NULL DEFAULT 'working',
                retention_expires_at TEXT,
                embedding   BLOB,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);
            CREATE INDEX IF NOT EXISTS idx_memories_key ON memories(key);

            -- FTS5 full-text search (BM25 scoring)
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                key, content, content=memories, content_rowid=rowid
            );

            -- FTS5 triggers: keep in sync with memories table
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, key, content)
                VALUES (new.rowid, new.key, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, key, content)
                VALUES ('delete', old.rowid, old.key, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, key, content)
                VALUES ('delete', old.rowid, old.key, old.content);
                INSERT INTO memories_fts(rowid, key, content)
                VALUES (new.rowid, new.key, new.content);
            END;

            -- Embedding cache with LRU eviction
            CREATE TABLE IF NOT EXISTS embedding_cache (
                content_hash TEXT PRIMARY KEY,
                embedding    BLOB NOT NULL,
                created_at   TEXT NOT NULL,
                accessed_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cache_accessed ON embedding_cache(accessed_at);",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_events (
                event_id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                slot_key TEXT NOT NULL,
                layer TEXT NOT NULL DEFAULT 'working',
                event_type TEXT NOT NULL,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL NOT NULL,
                importance REAL NOT NULL,
                provenance_source_class TEXT,
                provenance_reference TEXT,
                provenance_evidence_uri TEXT,
                retention_tier TEXT NOT NULL DEFAULT 'working',
                retention_expires_at TEXT,
                privacy_level TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                ingested_at TEXT NOT NULL,
                supersedes_event_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_memory_events_entity_slot
                ON memory_events(entity_id, slot_key, occurred_at DESC);

            CREATE TABLE IF NOT EXISTS belief_slots (
                entity_id TEXT NOT NULL,
                slot_key TEXT NOT NULL,
                value TEXT NOT NULL,
                status TEXT NOT NULL,
                winner_event_id TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL NOT NULL,
                importance REAL NOT NULL,
                privacy_level TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(entity_id, slot_key)
            );

            CREATE TABLE IF NOT EXISTS retrieval_docs (
                doc_id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                slot_key TEXT NOT NULL,
                text_body TEXT NOT NULL,
                layer TEXT NOT NULL DEFAULT 'working',
                provenance_source_class TEXT,
                provenance_reference TEXT,
                provenance_evidence_uri TEXT,
                retention_tier TEXT NOT NULL DEFAULT 'working',
                retention_expires_at TEXT,
                recency_score REAL NOT NULL,
                importance REAL NOT NULL,
                reliability REAL NOT NULL,
                contradiction_penalty REAL NOT NULL DEFAULT 0,
                visibility TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_retrieval_docs_entity ON retrieval_docs(entity_id);

            CREATE TABLE IF NOT EXISTS deletion_ledger (
                ledger_id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                target_slot_key TEXT NOT NULL,
                phase TEXT NOT NULL,
                reason TEXT NOT NULL,
                requested_by TEXT NOT NULL,
                executed_at TEXT NOT NULL
            );",
        )?;
        Self::run_schema_migrations(conn)?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn run_schema_migrations(conn: &Connection) -> anyhow::Result<()> {
        let version_table_exists = Self::table_exists(conn, "memory_schema_version")?;
        let memory_event_columns = Self::table_columns(conn, "memory_events")?;
        let memories_columns = Self::table_columns(conn, "memories")?;
        let retrieval_doc_columns = Self::table_columns(conn, "retrieval_docs")?;
        let pragma_user_version = Self::get_user_version(conn)?;

        let has_all_v2_columns = Self::MEMORY_EVENTS_V2_COLUMNS
            .iter()
            .all(|column| memory_event_columns.iter().any(|entry| entry == *column));
        let has_any_v2_columns = Self::MEMORY_EVENTS_V2_COLUMNS
            .iter()
            .any(|column| memory_event_columns.iter().any(|entry| entry == *column));
        let has_all_events_v3_columns = Self::MEMORY_EVENTS_V3_COLUMNS
            .iter()
            .all(|column| memory_event_columns.iter().any(|entry| entry == *column));
        let has_any_events_v3_columns = Self::MEMORY_EVENTS_V3_COLUMNS
            .iter()
            .any(|column| memory_event_columns.iter().any(|entry| entry == *column));
        let has_all_memories_v3_columns = Self::MEMORIES_V3_COLUMNS
            .iter()
            .all(|column| memories_columns.iter().any(|entry| entry == *column));
        let has_any_memories_v3_columns = Self::MEMORIES_V3_COLUMNS
            .iter()
            .any(|column| memories_columns.iter().any(|entry| entry == *column));
        let has_all_retrieval_v3_columns = Self::RETRIEVAL_DOCS_V3_COLUMNS
            .iter()
            .all(|column| retrieval_doc_columns.iter().any(|entry| entry == *column));
        let has_any_retrieval_v3_columns = Self::RETRIEVAL_DOCS_V3_COLUMNS
            .iter()
            .any(|column| retrieval_doc_columns.iter().any(|entry| entry == *column));

        if !version_table_exists {
            if has_any_v2_columns && !has_all_v2_columns {
                anyhow::bail!(
                    "sqlite schema inconsistent: memory_events has partial v2 columns without memory_schema_version"
                );
            }

            if pragma_user_version > Self::MEMORY_SCHEMA_V3 {
                anyhow::bail!(
                    "sqlite schema version unsupported: user_version={pragma_user_version}"
                );
            }

            if has_any_events_v3_columns != has_all_events_v3_columns {
                anyhow::bail!(
                    "sqlite schema inconsistent: memory_events has partial v3 retention columns without memory_schema_version"
                );
            }
            if has_any_memories_v3_columns != has_all_memories_v3_columns {
                anyhow::bail!(
                    "sqlite schema inconsistent: memories has partial v3 metadata columns without memory_schema_version"
                );
            }
            if has_any_retrieval_v3_columns != has_all_retrieval_v3_columns {
                anyhow::bail!(
                    "sqlite schema inconsistent: retrieval_docs has partial v3 metadata columns without memory_schema_version"
                );
            }

            Self::ensure_schema_version_table(conn)?;
            match pragma_user_version {
                0 => {
                    if has_all_v2_columns
                        && has_all_events_v3_columns
                        && has_all_memories_v3_columns
                        && has_all_retrieval_v3_columns
                    {
                        Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
                    } else if has_all_v2_columns {
                        Self::migrate_v2_to_v3(conn)?;
                        Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
                    } else {
                        Self::migrate_v1_to_v2(conn)?;
                        Self::migrate_v2_to_v3(conn)?;
                        Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
                    }
                }
                Self::MEMORY_SCHEMA_V1 => {
                    if has_all_v2_columns {
                        anyhow::bail!(
                            "sqlite schema inconsistent: PRAGMA user_version=1 but memory_events already has v2 columns"
                        );
                    }
                    Self::migrate_v1_to_v2(conn)?;
                    Self::migrate_v2_to_v3(conn)?;
                    Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
                }
                Self::MEMORY_SCHEMA_V2 => {
                    if !has_all_v2_columns {
                        let missing_columns =
                            Self::missing_v2_columns(&memory_event_columns).join(", ");
                        anyhow::bail!(
                            "sqlite schema inconsistent: user_version=2 but memory_events missing columns: {missing_columns}"
                        );
                    }
                    Self::migrate_v2_to_v3(conn)?;
                    Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
                }
                Self::MEMORY_SCHEMA_V3 => {
                    Self::validate_v3_columns(
                        &memory_event_columns,
                        &memories_columns,
                        &retrieval_doc_columns,
                        "user_version=3",
                    )?;
                    Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
                }
                other => anyhow::bail!("sqlite schema version unsupported: user_version={other}"),
            }
            Self::ensure_v3_indexes(conn)?;
            return Ok(());
        }

        let schema_version = Self::get_schema_version(conn)?;
        Self::validate_schema_markers(conn, schema_version, pragma_user_version)?;
        match schema_version {
            Self::MEMORY_SCHEMA_V1 => {
                Self::migrate_v1_to_v2(conn)?;
                Self::migrate_v2_to_v3(conn)?;
                Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
            }
            Self::MEMORY_SCHEMA_V2 => {
                if !has_all_v2_columns {
                    let missing_columns =
                        Self::missing_v2_columns(&memory_event_columns).join(", ");
                    anyhow::bail!(
                        "sqlite schema inconsistent: memory_schema_version=2 but memory_events missing columns: {missing_columns}"
                    );
                }
                Self::migrate_v2_to_v3(conn)?;
                Self::set_schema_version(conn, Self::MEMORY_SCHEMA_V3)?;
            }
            Self::MEMORY_SCHEMA_V3 => Self::validate_v3_columns(
                &memory_event_columns,
                &memories_columns,
                &retrieval_doc_columns,
                "memory_schema_version=3",
            )?,
            other => anyhow::bail!("sqlite schema version unsupported: {other}"),
        }

        Self::ensure_v3_indexes(conn)?;
        Ok(())
    }

    fn ensure_v3_indexes(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_memory_events_entity_layer
                 ON memory_events(entity_id, layer, occurred_at DESC);
             CREATE INDEX IF NOT EXISTS idx_memory_events_retention_expires
                 ON memory_events(retention_expires_at)
                 WHERE retention_expires_at IS NOT NULL;
             CREATE INDEX IF NOT EXISTS idx_retrieval_docs_entity_layer_visibility
                 ON retrieval_docs(entity_id, layer, visibility, updated_at DESC);
             CREATE INDEX IF NOT EXISTS idx_retrieval_docs_retention_expires
                 ON retrieval_docs(retention_expires_at)
                 WHERE retention_expires_at IS NOT NULL;",
        )?;
        Ok(())
    }

    fn migrate_v1_to_v2(conn: &Connection) -> anyhow::Result<()> {
        let memory_event_columns = Self::table_columns(conn, "memory_events")?;
        let normalized_columns: Vec<String> = memory_event_columns
            .iter()
            .map(|column| column.trim().to_ascii_lowercase())
            .collect();

        let has_all_v2_columns = Self::MEMORY_EVENTS_V2_COLUMNS.iter().all(|column| {
            normalized_columns
                .iter()
                .any(|entry| entry == &column.to_ascii_lowercase())
        });
        if has_all_v2_columns {
            return Ok(());
        }

        let has_any_v2_columns = Self::MEMORY_EVENTS_V2_COLUMNS.iter().any(|column| {
            normalized_columns
                .iter()
                .any(|entry| entry == &column.to_ascii_lowercase())
        });
        if has_any_v2_columns {
            anyhow::bail!(
                "sqlite schema inconsistent: memory_events migration requires all-or-none v2 columns"
            );
        }

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let mut migration_sql = String::new();
        if !normalized_columns
            .iter()
            .any(|column| column == &"layer".to_ascii_lowercase())
        {
            migration_sql.push_str(
                "ALTER TABLE memory_events ADD COLUMN layer TEXT NOT NULL DEFAULT 'working';\n",
            );
        }
        if !normalized_columns
            .iter()
            .any(|column| column == &"provenance_source_class".to_ascii_lowercase())
        {
            migration_sql
                .push_str("ALTER TABLE memory_events ADD COLUMN provenance_source_class TEXT;\n");
        }
        if !normalized_columns
            .iter()
            .any(|column| column == &"provenance_reference".to_ascii_lowercase())
        {
            migration_sql
                .push_str("ALTER TABLE memory_events ADD COLUMN provenance_reference TEXT;\n");
        }
        if !normalized_columns
            .iter()
            .any(|column| column == &"provenance_evidence_uri".to_ascii_lowercase())
        {
            migration_sql
                .push_str("ALTER TABLE memory_events ADD COLUMN provenance_evidence_uri TEXT;\n");
        }

        let migration_result = if migration_sql.is_empty() {
            Ok(())
        } else {
            conn.execute_batch(&migration_sql)
        };

        match migration_result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(err.into());
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn migrate_v2_to_v3(conn: &Connection) -> anyhow::Result<()> {
        let memory_event_columns: Vec<String> = Self::table_columns(conn, "memory_events")?
            .into_iter()
            .map(|column| column.to_ascii_lowercase())
            .collect();
        let memories_columns: Vec<String> = Self::table_columns(conn, "memories")?
            .into_iter()
            .map(|column| column.to_ascii_lowercase())
            .collect();
        let retrieval_doc_columns: Vec<String> = Self::table_columns(conn, "retrieval_docs")?
            .into_iter()
            .map(|column| column.to_ascii_lowercase())
            .collect();

        if Self::has_all_columns(&memory_event_columns, &Self::MEMORY_EVENTS_V3_COLUMNS)
            && Self::has_all_columns(&memories_columns, &Self::MEMORIES_V3_COLUMNS)
            && Self::has_all_columns(&retrieval_doc_columns, &Self::RETRIEVAL_DOCS_V3_COLUMNS)
        {
            return Ok(());
        }

        if Self::has_any_columns(&memory_event_columns, &Self::MEMORY_EVENTS_V3_COLUMNS)
            && !Self::has_all_columns(&memory_event_columns, &Self::MEMORY_EVENTS_V3_COLUMNS)
        {
            anyhow::bail!(
                "sqlite schema inconsistent: memory_events migration requires all-or-none v3 retention columns"
            );
        }
        if Self::has_any_columns(&memories_columns, &Self::MEMORIES_V3_COLUMNS)
            && !Self::has_all_columns(&memories_columns, &Self::MEMORIES_V3_COLUMNS)
        {
            anyhow::bail!(
                "sqlite schema inconsistent: memories migration requires all-or-none v3 metadata columns"
            );
        }
        if Self::has_any_columns(&retrieval_doc_columns, &Self::RETRIEVAL_DOCS_V3_COLUMNS)
            && !Self::has_all_columns(&retrieval_doc_columns, &Self::RETRIEVAL_DOCS_V3_COLUMNS)
        {
            anyhow::bail!(
                "sqlite schema inconsistent: retrieval_docs migration requires all-or-none v3 metadata columns"
            );
        }

        conn.execute_batch("BEGIN IMMEDIATE")?;
        let mut migration_sql = String::new();
        if !memory_event_columns
            .iter()
            .any(|column| column == "retention_tier")
        {
            migration_sql.push_str(
                "ALTER TABLE memory_events ADD COLUMN retention_tier TEXT NOT NULL DEFAULT 'working';\n",
            );
        }
        if !memory_event_columns
            .iter()
            .any(|column| column == "retention_expires_at")
        {
            migration_sql
                .push_str("ALTER TABLE memory_events ADD COLUMN retention_expires_at TEXT;\n");
        }
        if !memories_columns.iter().any(|column| column == "layer") {
            migration_sql.push_str(
                "ALTER TABLE memories ADD COLUMN layer TEXT NOT NULL DEFAULT 'working';\n",
            );
        }
        if !memories_columns
            .iter()
            .any(|column| column == "provenance_source_class")
        {
            migration_sql
                .push_str("ALTER TABLE memories ADD COLUMN provenance_source_class TEXT;\n");
        }
        if !memories_columns
            .iter()
            .any(|column| column == "provenance_reference")
        {
            migration_sql.push_str("ALTER TABLE memories ADD COLUMN provenance_reference TEXT;\n");
        }
        if !memories_columns
            .iter()
            .any(|column| column == "provenance_evidence_uri")
        {
            migration_sql
                .push_str("ALTER TABLE memories ADD COLUMN provenance_evidence_uri TEXT;\n");
        }
        if !memories_columns
            .iter()
            .any(|column| column == "retention_tier")
        {
            migration_sql.push_str(
                "ALTER TABLE memories ADD COLUMN retention_tier TEXT NOT NULL DEFAULT 'working';\n",
            );
        }
        if !memories_columns
            .iter()
            .any(|column| column == "retention_expires_at")
        {
            migration_sql.push_str("ALTER TABLE memories ADD COLUMN retention_expires_at TEXT;\n");
        }
        if !retrieval_doc_columns.iter().any(|column| column == "layer") {
            migration_sql.push_str(
                "ALTER TABLE retrieval_docs ADD COLUMN layer TEXT NOT NULL DEFAULT 'working';\n",
            );
        }
        if !retrieval_doc_columns
            .iter()
            .any(|column| column == "provenance_source_class")
        {
            migration_sql
                .push_str("ALTER TABLE retrieval_docs ADD COLUMN provenance_source_class TEXT;\n");
        }
        if !retrieval_doc_columns
            .iter()
            .any(|column| column == "provenance_reference")
        {
            migration_sql
                .push_str("ALTER TABLE retrieval_docs ADD COLUMN provenance_reference TEXT;\n");
        }
        if !retrieval_doc_columns
            .iter()
            .any(|column| column == "provenance_evidence_uri")
        {
            migration_sql
                .push_str("ALTER TABLE retrieval_docs ADD COLUMN provenance_evidence_uri TEXT;\n");
        }
        if !retrieval_doc_columns
            .iter()
            .any(|column| column == "retention_tier")
        {
            migration_sql.push_str("ALTER TABLE retrieval_docs ADD COLUMN retention_tier TEXT NOT NULL DEFAULT 'working';\n");
        }
        if !retrieval_doc_columns
            .iter()
            .any(|column| column == "retention_expires_at")
        {
            migration_sql
                .push_str("ALTER TABLE retrieval_docs ADD COLUMN retention_expires_at TEXT;\n");
        }

        migration_sql.push_str(concat!(
            "CREATE INDEX IF NOT EXISTS idx_memory_events_entity_layer\n",
            "                ON memory_events(entity_id, layer, occurred_at DESC);\n",
            "CREATE INDEX IF NOT EXISTS idx_memory_events_retention_expires\n",
            "                ON memory_events(retention_expires_at)\n",
            "                WHERE retention_expires_at IS NOT NULL;\n",
            "CREATE INDEX IF NOT EXISTS idx_retrieval_docs_entity_layer_visibility\n",
            "                ON retrieval_docs(entity_id, layer, visibility, updated_at DESC);\n",
            "CREATE INDEX IF NOT EXISTS idx_retrieval_docs_retention_expires\n",
            "                ON retrieval_docs(retention_expires_at)\n",
            "                WHERE retention_expires_at IS NOT NULL;\n",
        ));

        let migration_result = if migration_sql.is_empty() {
            Ok(())
        } else {
            conn.execute_batch(&migration_sql)
        };

        match migration_result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(err) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(err.into());
            }
        }

        Ok(())
    }

    fn table_exists(conn: &Connection, table_name: &str) -> anyhow::Result<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![table_name],
            |row| row.get(0),
        )?;
        Ok(count == 1)
    }

    fn table_columns(conn: &Connection, table_name: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table_name})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = Vec::new();
        for row in rows {
            columns.push(row?);
        }
        Ok(columns)
    }

    fn missing_v2_columns(columns: &[String]) -> Vec<&'static str> {
        Self::MEMORY_EVENTS_V2_COLUMNS
            .iter()
            .copied()
            .filter(|column| !columns.iter().any(|entry| entry == column))
            .collect()
    }

    fn missing_columns<'a>(columns: &[String], required: &'a [&'a str]) -> Vec<&'a str> {
        required
            .iter()
            .copied()
            .filter(|column| !columns.iter().any(|entry| entry == column))
            .collect()
    }

    fn has_all_columns(columns: &[String], required: &[&str]) -> bool {
        required
            .iter()
            .all(|column| columns.iter().any(|entry| entry == *column))
    }

    fn has_any_columns(columns: &[String], required: &[&str]) -> bool {
        required
            .iter()
            .any(|column| columns.iter().any(|entry| entry == *column))
    }

    fn validate_v3_columns(
        memory_event_columns: &[String],
        memories_columns: &[String],
        retrieval_doc_columns: &[String],
        state: &str,
    ) -> anyhow::Result<()> {
        let missing_event_v3 =
            Self::missing_columns(memory_event_columns, &Self::MEMORY_EVENTS_V3_COLUMNS);
        if !missing_event_v3.is_empty() {
            anyhow::bail!(
                "sqlite schema inconsistent: {state} but memory_events missing v3 columns: {}",
                missing_event_v3.join(", ")
            );
        }

        let missing_memories_v3 =
            Self::missing_columns(memories_columns, &Self::MEMORIES_V3_COLUMNS);
        if !missing_memories_v3.is_empty() {
            anyhow::bail!(
                "sqlite schema inconsistent: {state} but memories missing v3 columns: {}",
                missing_memories_v3.join(", ")
            );
        }

        let missing_retrieval_v3 =
            Self::missing_columns(retrieval_doc_columns, &Self::RETRIEVAL_DOCS_V3_COLUMNS);
        if !missing_retrieval_v3.is_empty() {
            anyhow::bail!(
                "sqlite schema inconsistent: {state} but retrieval_docs missing v3 columns: {}",
                missing_retrieval_v3.join(", ")
            );
        }

        Ok(())
    }

    fn ensure_schema_version_table(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_schema_version (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                version INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    fn validate_schema_markers(
        conn: &Connection,
        app_version: i64,
        pragma_version: i64,
    ) -> anyhow::Result<()> {
        if pragma_version == 0 {
            Self::set_user_version(conn, app_version)?;
            return Ok(());
        }

        if pragma_version != app_version {
            anyhow::bail!(
                "sqlite schema inconsistent: memory_schema_version={app_version} but PRAGMA user_version={pragma_version}"
            );
        }

        Ok(())
    }

    fn get_schema_version(conn: &Connection) -> anyhow::Result<i64> {
        let row_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_schema_version", [], |row| {
                row.get(0)
            })?;

        if row_count != 1 {
            anyhow::bail!(
                "sqlite schema inconsistent: memory_schema_version must contain exactly one row"
            );
        }

        let version: i64 = conn.query_row(
            "SELECT version FROM memory_schema_version WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    fn set_schema_version(conn: &Connection, version: i64) -> anyhow::Result<()> {
        let now = Local::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memory_schema_version (id, version, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET
                 version = excluded.version,
                 updated_at = excluded.updated_at",
            params![version, now],
        )?;
        Self::set_user_version(conn, version)?;
        Ok(())
    }

    fn get_user_version(conn: &Connection) -> anyhow::Result<i64> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        Ok(version)
    }

    fn set_user_version(conn: &Connection, version: i64) -> anyhow::Result<()> {
        conn.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        Ok(())
    }

    fn category_to_str(cat: &MemoryCategory) -> String {
        match cat {
            MemoryCategory::Core => "core".into(),
            MemoryCategory::Daily => "daily".into(),
            MemoryCategory::Conversation => "conversation".into(),
            MemoryCategory::Custom(name) => name.clone(),
        }
    }

    fn str_to_category(s: &str) -> MemoryCategory {
        match s {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }

    fn source_to_str(source: &MemorySource) -> &'static str {
        match source {
            MemorySource::ExplicitUser => "explicit_user",
            MemorySource::ToolVerified => "tool_verified",
            MemorySource::System => "system",
            MemorySource::Inferred => "inferred",
        }
    }

    fn layer_to_str(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    fn retention_tier_for_layer(layer: &MemoryLayer) -> &'static str {
        match layer {
            MemoryLayer::Working => "working",
            MemoryLayer::Episodic => "episodic",
            MemoryLayer::Semantic => "semantic",
            MemoryLayer::Procedural => "procedural",
            MemoryLayer::Identity => "identity",
        }
    }

    fn retention_expiry_for_layer(layer: &MemoryLayer, occurred_at: &str) -> Option<String> {
        let retention_days = match layer {
            MemoryLayer::Working => Some(2),
            MemoryLayer::Episodic => Some(30),
            MemoryLayer::Semantic | MemoryLayer::Procedural | MemoryLayer::Identity => None,
        }?;

        chrono::DateTime::parse_from_rfc3339(occurred_at)
            .ok()
            .map(|ts| (ts + chrono::Duration::days(retention_days)).to_rfc3339())
    }

    fn str_to_source(source: &str) -> MemorySource {
        match source {
            "explicit_user" => MemorySource::ExplicitUser,
            "tool_verified" => MemorySource::ToolVerified,
            "inferred" => MemorySource::Inferred,
            _ => MemorySource::System,
        }
    }

    fn privacy_to_str(level: &PrivacyLevel) -> &'static str {
        match level {
            PrivacyLevel::Public => "public",
            PrivacyLevel::Private => "private",
            PrivacyLevel::Secret => "secret",
        }
    }

    fn str_to_privacy(level: &str) -> PrivacyLevel {
        match level {
            "public" => PrivacyLevel::Public,
            "secret" => PrivacyLevel::Secret,
            _ => PrivacyLevel::Private,
        }
    }

    fn source_priority(source: &MemorySource) -> u8 {
        match source {
            MemorySource::ExplicitUser => 4,
            MemorySource::ToolVerified => 3,
            MemorySource::System => 2,
            MemorySource::Inferred => 1,
        }
    }

    fn compare_normalized_timestamps(incoming: &str, incumbent: &str) -> std::cmp::Ordering {
        let incoming_normalized = chrono::DateTime::parse_from_rfc3339(incoming)
            .ok()
            .and_then(|parsed| parsed.timestamp_nanos_opt());
        let incumbent_normalized = chrono::DateTime::parse_from_rfc3339(incumbent)
            .ok()
            .and_then(|parsed| parsed.timestamp_nanos_opt());

        match (incoming_normalized, incumbent_normalized) {
            (Some(incoming), Some(incumbent)) => incoming.cmp(&incumbent),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        }
    }

    fn contradiction_penalty(confidence: f64, importance: f64) -> f64 {
        let confidence = confidence.clamp(0.0, 1.0);
        let importance = importance.clamp(0.0, 1.0);
        (0.12 + 0.10 * confidence + 0.08 * importance).clamp(0.0, 1.0)
    }

    /// Deterministic content hash for embedding cache.
    /// Uses SHA-256 (truncated) instead of `DefaultHasher`, which is
    /// explicitly documented as unstable across Rust versions.
    fn content_hash(text: &str) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(text.as_bytes());
        // First 8 bytes → 16 hex chars, matching previous format length
        format!(
            "{:016x}",
            u64::from_be_bytes(
                hash[..8]
                    .try_into()
                    .expect("SHA-256 always produces >= 8 bytes")
            )
        )
    }

    /// Get embedding from cache, or compute + cache it
    async fn get_or_compute_embedding(&self, text: &str) -> anyhow::Result<Option<Vec<f32>>> {
        if self.embedder.dimensions() == 0 {
            return Ok(None); // Noop embedder
        }

        let hash = Self::content_hash(text);
        let now = Local::now().to_rfc3339();

        // Check cache
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            let mut stmt =
                conn.prepare("SELECT embedding FROM embedding_cache WHERE content_hash = ?1")?;
            let cached: Option<Vec<u8>> = stmt.query_row(params![hash], |row| row.get(0)).ok();

            if let Some(bytes) = cached {
                // Update accessed_at for LRU
                conn.execute(
                    "UPDATE embedding_cache SET accessed_at = ?1 WHERE content_hash = ?2",
                    params![now, hash],
                )?;
                return Ok(Some(vector::bytes_to_vec(&bytes)));
            }
        }

        // Compute embedding
        let embedding = self.embedder.embed_one(text).await?;
        let bytes = vector::vec_to_bytes(&embedding);

        // Store in cache + LRU eviction
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            conn.execute(
                "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, created_at, accessed_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![hash, bytes, now, now],
            )?;

            // LRU eviction: keep only cache_max entries
            #[allow(clippy::cast_possible_wrap)]
            let max = self.cache_max as i64;
            conn.execute(
                "DELETE FROM embedding_cache WHERE content_hash IN (
                    SELECT content_hash FROM embedding_cache
                    ORDER BY accessed_at ASC
                    LIMIT MAX(0, (SELECT COUNT(*) FROM embedding_cache) - ?1)
                )",
                params![max],
            )?;
        }

        Ok(Some(embedding))
    }

    /// FTS5 BM25 keyword search
    fn fts5_search(
        conn: &Connection,
        query: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        // Escape FTS5 special chars and build query
        let fts_query: String = query
            .split_whitespace()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let sql = "SELECT m.id, bm25(memories_fts) as score
                   FROM memories_fts f
                   JOIN memories m ON m.rowid = f.rowid
                   WHERE memories_fts MATCH ?1
                   ORDER BY score
                   LIMIT ?2";

        let mut stmt = conn.prepare(sql)?;
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = limit as i64;

        let rows = stmt.query_map(params![fts_query, limit_i64], |row| {
            let id: String = row.get(0)?;
            let score: f64 = row.get(1)?;
            // BM25 returns negative scores (lower = better), negate for ranking
            #[allow(clippy::cast_possible_truncation)]
            Ok((id, (-score) as f32))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Vector similarity search: scan embeddings and compute cosine similarity
    fn vector_search(
        conn: &Connection,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<Vec<(String, f32)>> {
        let mut stmt =
            conn.prepare("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL")?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            Ok((id, blob))
        })?;

        let mut scored: Vec<(String, f32)> = Vec::new();
        for row in rows {
            let (id, blob) = row?;
            let emb = vector::bytes_to_vec(&blob);
            let sim = vector::cosine_similarity(query_embedding, &emb);
            if sim > 0.0 {
                scored.push((id, sim));
            }
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    /// Safe reindex: rebuild FTS5 + embeddings with rollback on failure
    #[allow(dead_code)]
    pub async fn reindex(&self) -> anyhow::Result<usize> {
        // Step 1: Rebuild FTS5
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            conn.execute_batch("INSERT INTO memories_fts(memories_fts) VALUES('rebuild');")?;
        }

        // Step 2: Re-embed all memories that lack embeddings
        if self.embedder.dimensions() == 0 {
            return Ok(0);
        }

        let entries: Vec<(String, String)> = {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;

            let mut stmt =
                conn.prepare("SELECT id, content FROM memories WHERE embedding IS NULL")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.filter_map(std::result::Result::ok).collect()
        };

        let mut count = 0;
        for (id, content) in &entries {
            if let Ok(Some(emb)) = self.get_or_compute_embedding(content).await {
                let bytes = vector::vec_to_bytes(&emb);
                let conn = self
                    .conn
                    .lock()
                    .map_err(|e| anyhow::anyhow!("Lock error: {e}"))?;
                conn.execute(
                    "UPDATE memories SET embedding = ?1 WHERE id = ?2",
                    params![bytes, id],
                )?;
                count += 1;
            }
        }

        Ok(count)
    }
}

#[allow(clippy::too_many_lines)]
#[allow(clippy::unused_self, clippy::unused_async)]
impl SqliteMemory {
    fn name(&self) -> &str {
        "sqlite"
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    fn name(&self) -> &str {
        SqliteMemory::name(self)
    }

    async fn health_check(&self) -> bool {
        SqliteMemory::health_check(self).await
    }

    async fn append_event(&self, input: MemoryEventInput) -> anyhow::Result<MemoryEvent> {
        SqliteMemory::append_event(self, input).await
    }

    async fn recall_scoped(&self, query: RecallQuery) -> anyhow::Result<Vec<MemoryRecallItem>> {
        SqliteMemory::recall_scoped(self, query).await
    }

    async fn resolve_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
    ) -> anyhow::Result<Option<BeliefSlot>> {
        SqliteMemory::resolve_slot(self, entity_id, slot_key).await
    }

    async fn forget_slot(
        &self,
        entity_id: &str,
        slot_key: &str,
        mode: ForgetMode,
        reason: &str,
    ) -> anyhow::Result<ForgetOutcome> {
        SqliteMemory::forget_slot(self, entity_id, slot_key, mode, reason).await
    }

    async fn count_events(&self, entity_id: Option<&str>) -> anyhow::Result<usize> {
        SqliteMemory::count_events(self, entity_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MemoryEventType, MemoryInferenceEvent};
    use chrono::{Duration, Utc};
    use rusqlite::Connection;
    use std::fs;
    use tempfile::TempDir;

    fn temp_sqlite() -> (TempDir, SqliteMemory) {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();
        (tmp, mem)
    }

    fn legacy_db_path(tmp: &TempDir) -> PathBuf {
        tmp.path().join("memory").join("brain.db")
    }

    fn seed_legacy_v1_db(tmp: &TempDir) {
        let db_path = legacy_db_path(tmp);
        fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memories (
                id          TEXT PRIMARY KEY,
                key         TEXT NOT NULL UNIQUE,
                content     TEXT NOT NULL,
                category    TEXT NOT NULL DEFAULT 'core',
                embedding   BLOB,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE memory_events (
                event_id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                slot_key TEXT NOT NULL,
                event_type TEXT NOT NULL,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL NOT NULL,
                importance REAL NOT NULL,
                privacy_level TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                ingested_at TEXT NOT NULL,
                supersedes_event_id TEXT
            );

            INSERT INTO memories (id, key, content, category, embedding, created_at, updated_at)
            VALUES ('m1', 'legacy_key', 'legacy content', 'core', NULL, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z');

            INSERT INTO memory_events (
                event_id, entity_id, slot_key, event_type, value, source,
                confidence, importance, privacy_level, occurred_at, ingested_at, supersedes_event_id
            ) VALUES (
                'e1', 'tenant_1', 'legacy.slot', 'fact_added', 'legacy event', 'system',
                0.8, 0.5, 'private', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', NULL
            );",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn sqlite_name() {
        let (_tmp, mem) = temp_sqlite();
        assert_eq!(mem.name(), "sqlite");
    }

    #[tokio::test]
    async fn sqlite_health() {
        let (_tmp, mem) = temp_sqlite();
        assert!(mem.health_check().await);
    }

    #[tokio::test]
    async fn sqlite_store_and_get() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("user_lang", "Prefers Rust", MemoryCategory::Core)
            .await
            .unwrap();

        let entry = mem.fetch_projection_entry("user_lang").await.unwrap();
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.key, "user_lang");
        assert_eq!(entry.content, "Prefers Rust");
        assert_eq!(entry.category, MemoryCategory::Core);
    }

    #[tokio::test]
    async fn sqlite_store_upsert() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("pref", "likes Rust", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("pref", "loves Rust", MemoryCategory::Core)
            .await
            .unwrap();

        let entry = mem.fetch_projection_entry("pref").await.unwrap().unwrap();
        assert_eq!(entry.content, "loves Rust");
        assert_eq!(mem.count_projection_entries().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn sqlite_recall_keyword() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "Rust is fast and safe", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("b", "Python is interpreted", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("c", "Rust has zero-cost abstractions", MemoryCategory::Core)
            .await
            .unwrap();

        let results = mem.search_projection("Rust", 10).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results
            .iter()
            .all(|r| r.content.to_lowercase().contains("rust")));
    }

    #[tokio::test]
    async fn sqlite_recall_multi_keyword() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "Rust is fast", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("b", "Rust is safe and fast", MemoryCategory::Core)
            .await
            .unwrap();

        let results = mem.search_projection("fast safe", 10).await.unwrap();
        assert!(!results.is_empty());
        // Entry with both keywords should score higher
        assert!(results[0].content.contains("safe") && results[0].content.contains("fast"));
    }

    #[tokio::test]
    async fn sqlite_recall_no_match() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "Rust rocks", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("javascript", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn sqlite_forget() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("temp", "temporary data", MemoryCategory::Conversation)
            .await
            .unwrap();
        assert_eq!(mem.count_projection_entries().await.unwrap(), 1);

        let removed = mem.delete_projection_entry("temp").await.unwrap();
        assert!(removed);
        assert_eq!(mem.count_projection_entries().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn sqlite_forget_nonexistent() {
        let (_tmp, mem) = temp_sqlite();
        let removed = mem.delete_projection_entry("nope").await.unwrap();
        assert!(!removed);
    }

    #[tokio::test]
    async fn sqlite_list_all() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "one", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("b", "two", MemoryCategory::Daily)
            .await
            .unwrap();
        mem.upsert_projection_entry("c", "three", MemoryCategory::Conversation)
            .await
            .unwrap();

        let all = mem.list_projection_entries(None).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn sqlite_list_by_category() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "core1", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("b", "core2", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("c", "daily1", MemoryCategory::Daily)
            .await
            .unwrap();

        let core = mem
            .list_projection_entries(Some(&MemoryCategory::Core))
            .await
            .unwrap();
        assert_eq!(core.len(), 2);

        let daily = mem
            .list_projection_entries(Some(&MemoryCategory::Daily))
            .await
            .unwrap();
        assert_eq!(daily.len(), 1);
    }

    #[tokio::test]
    async fn sqlite_count_empty() {
        let (_tmp, mem) = temp_sqlite();
        assert_eq!(mem.count_projection_entries().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn sqlite_get_nonexistent() {
        let (_tmp, mem) = temp_sqlite();
        assert!(mem.fetch_projection_entry("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn sqlite_db_persists() {
        let tmp = TempDir::new().unwrap();

        {
            let mem = SqliteMemory::new(tmp.path()).unwrap();
            mem.upsert_projection_entry("persist", "I survive restarts", MemoryCategory::Core)
                .await
                .unwrap();
        }

        // Reopen
        let mem2 = SqliteMemory::new(tmp.path()).unwrap();
        let entry = mem2.fetch_projection_entry("persist").await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "I survive restarts");
    }

    #[tokio::test]
    async fn sqlite_category_roundtrip() {
        let (_tmp, mem) = temp_sqlite();
        let categories = [
            MemoryCategory::Core,
            MemoryCategory::Daily,
            MemoryCategory::Conversation,
            MemoryCategory::Custom("project".into()),
        ];

        for (i, cat) in categories.iter().enumerate() {
            mem.upsert_projection_entry(&format!("k{i}"), &format!("v{i}"), cat.clone())
                .await
                .unwrap();
        }

        for (i, cat) in categories.iter().enumerate() {
            let entry = mem
                .fetch_projection_entry(&format!("k{i}"))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&entry.category, cat);
        }
    }

    // ── FTS5 search tests ────────────────────────────────────────

    #[tokio::test]
    async fn fts5_bm25_ranking() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry(
            "a",
            "Rust is a systems programming language",
            MemoryCategory::Core,
        )
        .await
        .unwrap();
        mem.upsert_projection_entry("b", "Python is great for scripting", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry(
            "c",
            "Rust and Rust and Rust everywhere",
            MemoryCategory::Core,
        )
        .await
        .unwrap();

        let results = mem.search_projection("Rust", 10).await.unwrap();
        assert!(results.len() >= 2);
        // All results should contain "Rust"
        for r in &results {
            assert!(
                r.content.to_lowercase().contains("rust"),
                "Expected 'rust' in: {}",
                r.content
            );
        }
    }

    #[tokio::test]
    async fn fts5_multi_word_query() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "The quick brown fox jumps", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("b", "A lazy dog sleeps", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("c", "The quick dog runs fast", MemoryCategory::Core)
            .await
            .unwrap();

        let results = mem.search_projection("quick dog", 10).await.unwrap();
        assert!(!results.is_empty());
        // "The quick dog runs fast" matches both terms
        assert!(results[0].content.contains("quick"));
    }

    #[tokio::test]
    async fn recall_empty_query_returns_empty() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "data", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn recall_whitespace_query_returns_empty() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "data", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("   ", 10).await.unwrap();
        assert!(results.is_empty());
    }

    // ── Embedding cache tests ────────────────────────────────────

    #[test]
    fn content_hash_deterministic() {
        let h1 = SqliteMemory::content_hash("hello world");
        let h2 = SqliteMemory::content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_different_inputs() {
        let h1 = SqliteMemory::content_hash("hello");
        let h2 = SqliteMemory::content_hash("world");
        assert_ne!(h1, h2);
    }

    // ── Schema tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn schema_has_fts5_table() {
        let (_tmp, mem) = temp_sqlite();
        let conn = mem.conn.lock().unwrap();
        // FTS5 table should exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memories_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn schema_has_embedding_cache() {
        let (_tmp, mem) = temp_sqlite();
        let conn = mem.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='embedding_cache'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn schema_memories_has_embedding_column() {
        let (_tmp, mem) = temp_sqlite();
        let conn = mem.conn.lock().unwrap();
        // Check that embedding column exists by querying it
        let result = conn.execute_batch("SELECT embedding FROM memories LIMIT 0");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sqlite_schema_migrates_from_v1_to_v2() {
        let tmp = TempDir::new().unwrap();
        seed_legacy_v1_db(&tmp);

        let mem = SqliteMemory::new(tmp.path()).unwrap();
        let legacy_projection = mem.fetch_projection_entry("legacy_key").await.unwrap();
        assert!(
            legacy_projection.is_some(),
            "legacy row should remain readable"
        );
        assert_eq!(legacy_projection.unwrap().content, "legacy content");

        let conn = mem.conn.lock().unwrap();
        let migration_event: (String, Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT layer, provenance_source_class, provenance_reference, provenance_evidence_uri FROM memory_events WHERE event_id = 'e1'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        let (layer, source_class, reference, evidence_uri) = migration_event;
        assert_eq!(
            layer, "working",
            "legacy events should default layer to working"
        );
        assert!(
            source_class.is_none(),
            "legacy provenance source should be null"
        );
        assert!(
            reference.is_none(),
            "legacy provenance reference should be null"
        );
        assert!(
            evidence_uri.is_none(),
            "legacy provenance evidence URI should be null"
        );
        let schema_version: i64 = conn
            .query_row(
                "SELECT version FROM memory_schema_version WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(schema_version, SqliteMemory::MEMORY_SCHEMA_V3);

        let columns = SqliteMemory::table_columns(&conn, "memory_events").unwrap();
        for expected_column in SqliteMemory::MEMORY_EVENTS_V2_COLUMNS {
            assert!(
                columns.contains(&expected_column.to_string()),
                "missing migrated column: {expected_column}"
            );
        }
        for expected_column in SqliteMemory::MEMORY_EVENTS_V3_COLUMNS {
            assert!(
                columns.contains(&expected_column.to_string()),
                "missing migrated column: {expected_column}"
            );
        }

        let memory_columns = SqliteMemory::table_columns(&conn, "memories").unwrap();
        for expected_column in SqliteMemory::MEMORIES_V3_COLUMNS {
            assert!(
                memory_columns.contains(&expected_column.to_string()),
                "missing migrated memories column: {expected_column}"
            );
        }

        let retrieval_columns = SqliteMemory::table_columns(&conn, "retrieval_docs").unwrap();
        for expected_column in SqliteMemory::RETRIEVAL_DOCS_V3_COLUMNS {
            assert!(
                retrieval_columns.contains(&expected_column.to_string()),
                "missing migrated retrieval_docs column: {expected_column}"
            );
        }
    }

    #[tokio::test]
    async fn sqlite_schema_rollback_guard() {
        let tmp = TempDir::new().unwrap();
        seed_legacy_v1_db(&tmp);

        let db_path = legacy_db_path(&tmp);
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memory_schema_version (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                version INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO memory_schema_version (id, version, updated_at)
            VALUES (1, 2, '2024-01-01T00:00:00Z');",
        )
        .unwrap();
        drop(conn);

        let err = SqliteMemory::new(tmp.path())
            .err()
            .expect("expected migration consistency error")
            .to_string();
        assert!(
            err.contains(
                "sqlite schema inconsistent: memory_schema_version=2 but memory_events missing columns"
            ),
            "unexpected error: {err}"
        );

        let verify_conn = Connection::open(&db_path).unwrap();
        let memories_count: i64 = verify_conn
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            memories_count, 1,
            "rollback guard should preserve existing rows"
        );
        let events_count: i64 = verify_conn
            .query_row("SELECT COUNT(*) FROM memory_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            events_count, 1,
            "rollback guard should preserve legacy events"
        );
    }

    #[tokio::test]
    async fn sqlite_schema_rejects_inconsistent_state() {
        let tmp = TempDir::new().unwrap();
        let db_path = legacy_db_path(&tmp);
        fs::create_dir_all(db_path.parent().unwrap()).unwrap();

        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE memory_events (
                event_id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                slot_key TEXT NOT NULL,
                layer TEXT NOT NULL DEFAULT 'working',
                event_type TEXT NOT NULL,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL NOT NULL,
                importance REAL NOT NULL,
                privacy_level TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                ingested_at TEXT NOT NULL,
                supersedes_event_id TEXT
            );",
        )
        .unwrap();
        drop(conn);

        let err = SqliteMemory::new(tmp.path())
            .err()
            .expect("expected migration inconsistency error")
            .to_string();
        assert!(
            err.contains(
                "sqlite schema inconsistent: memory_events has partial v2 columns without memory_schema_version"
            ),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn sqlite_schema_syncs_user_version_marker() {
        let tmp = TempDir::new().unwrap();
        seed_legacy_v1_db(&tmp);

        let mem = SqliteMemory::new(tmp.path()).unwrap();
        let conn = mem.conn.lock().unwrap();
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(user_version, SqliteMemory::MEMORY_SCHEMA_V3);
    }

    #[tokio::test]
    async fn sqlite_schema_rejects_user_version_mismatch() {
        let tmp = TempDir::new().unwrap();
        seed_legacy_v1_db(&tmp);

        {
            let mem = SqliteMemory::new(tmp.path()).unwrap();
            let conn = mem.conn.lock().unwrap();
            conn.execute_batch("PRAGMA user_version = 1").unwrap();
        }

        let err = SqliteMemory::new(tmp.path())
            .err()
            .expect("expected user_version mismatch error")
            .to_string();
        assert!(
            err.contains(
                "sqlite schema inconsistent: memory_schema_version=3 but PRAGMA user_version=1"
            ),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn sqlite_schema_rejects_unsupported_user_version() {
        let tmp = TempDir::new().unwrap();
        seed_legacy_v1_db(&tmp);

        let db_path = legacy_db_path(&tmp);
        let conn = Connection::open(db_path).unwrap();
        conn.execute_batch("PRAGMA user_version = 999").unwrap();
        drop(conn);

        let err = SqliteMemory::new(tmp.path())
            .err()
            .expect("expected unsupported user_version error")
            .to_string();
        assert!(
            err.contains("sqlite schema version unsupported: user_version=999"),
            "unexpected error: {err}"
        );
    }

    // ── FTS5 sync trigger tests ──────────────────────────────────

    #[tokio::test]
    async fn fts5_syncs_on_insert() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("test_key", "unique_searchterm_xyz", MemoryCategory::Core)
            .await
            .unwrap();

        let conn = mem.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH '\"unique_searchterm_xyz\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn fts5_syncs_on_delete() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("del_key", "deletable_content_abc", MemoryCategory::Core)
            .await
            .unwrap();
        mem.delete_projection_entry("del_key").await.unwrap();

        let conn = mem.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH '\"deletable_content_abc\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn fts5_syncs_on_update() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("upd_key", "original_content_111", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("upd_key", "updated_content_222", MemoryCategory::Core)
            .await
            .unwrap();

        let conn = mem.conn.lock().unwrap();
        // Old content should not be findable
        let old: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH '\"original_content_111\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old, 0);

        // New content should be findable
        let new: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH '\"updated_content_222\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new, 1);
    }

    // ── With-embedder constructor test ───────────────────────────

    #[test]
    fn with_embedder_noop() {
        let tmp = TempDir::new().unwrap();
        let embedder = Arc::new(super::super::embeddings::NoopEmbedding);
        let mem = SqliteMemory::with_embedder(tmp.path(), embedder, 0.7, 0.3, 1000);
        assert!(mem.is_ok());
        assert_eq!(mem.unwrap().name(), "sqlite");
    }

    // ── Reindex test ─────────────────────────────────────────────

    #[tokio::test]
    async fn reindex_rebuilds_fts() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("r1", "reindex test alpha", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("r2", "reindex test beta", MemoryCategory::Core)
            .await
            .unwrap();

        // Reindex should succeed (noop embedder → 0 re-embedded)
        let count = mem.reindex().await.unwrap();
        assert_eq!(count, 0);

        // FTS should still work after rebuild
        let results = mem.search_projection("reindex", 10).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    // ── Recall limit test ────────────────────────────────────────

    #[tokio::test]
    async fn recall_respects_limit() {
        let (_tmp, mem) = temp_sqlite();
        for i in 0..20 {
            mem.upsert_projection_entry(
                &format!("k{i}"),
                &format!("common keyword item {i}"),
                MemoryCategory::Core,
            )
            .await
            .unwrap();
        }

        let results = mem.search_projection("common keyword", 5).await.unwrap();
        assert!(results.len() <= 5);
    }

    // ── Score presence test ──────────────────────────────────────

    #[tokio::test]
    async fn recall_results_have_scores() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("s1", "scored result test", MemoryCategory::Core)
            .await
            .unwrap();

        let results = mem.search_projection("scored", 10).await.unwrap();
        assert!(!results.is_empty());
        for r in &results {
            assert!(r.score.is_some(), "Expected score on result: {:?}", r.key);
        }
    }

    // ── Edge cases: FTS5 special characters ──────────────────────

    #[tokio::test]
    async fn recall_with_quotes_in_query() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("q1", "He said hello world", MemoryCategory::Core)
            .await
            .unwrap();
        // Quotes in query should not crash FTS5
        let results = mem.search_projection("\"hello\"", 10).await.unwrap();
        // May or may not match depending on FTS5 escaping, but must not error
        assert!(results.len() <= 10);
    }

    #[tokio::test]
    async fn recall_with_asterisk_in_query() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a1", "wildcard test content", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("wild*", 10).await.unwrap();
        assert!(results.len() <= 10);
    }

    #[tokio::test]
    async fn recall_with_parentheses_in_query() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("p1", "function call test", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("function()", 10).await.unwrap();
        assert!(results.len() <= 10);
    }

    #[tokio::test]
    async fn recall_with_sql_injection_attempt() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("safe", "normal content", MemoryCategory::Core)
            .await
            .unwrap();
        // Should not crash or leak data
        let results = mem
            .search_projection("'; DROP TABLE memories; --", 10)
            .await
            .unwrap();
        assert!(results.len() <= 10);
        // Table should still exist
        assert_eq!(mem.count_projection_entries().await.unwrap(), 1);
    }

    // ── Edge cases: store ────────────────────────────────────────

    #[tokio::test]
    async fn store_empty_content() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("empty", "", MemoryCategory::Core)
            .await
            .unwrap();
        let entry = mem.fetch_projection_entry("empty").await.unwrap().unwrap();
        assert_eq!(entry.content, "");
    }

    #[tokio::test]
    async fn store_empty_key() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("", "content for empty key", MemoryCategory::Core)
            .await
            .unwrap();
        let entry = mem.fetch_projection_entry("").await.unwrap().unwrap();
        assert_eq!(entry.content, "content for empty key");
    }

    #[tokio::test]
    async fn store_very_long_content() {
        let (_tmp, mem) = temp_sqlite();
        let long_content = "x".repeat(100_000);
        mem.upsert_projection_entry("long", &long_content, MemoryCategory::Core)
            .await
            .unwrap();
        let entry = mem.fetch_projection_entry("long").await.unwrap().unwrap();
        assert_eq!(entry.content.len(), 100_000);
    }

    #[tokio::test]
    async fn store_unicode_and_emoji() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("emoji_key_🦀", "こんにちは 🚀 Ñoño", MemoryCategory::Core)
            .await
            .unwrap();
        let entry = mem
            .fetch_projection_entry("emoji_key_🦀")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.content, "こんにちは 🚀 Ñoño");
    }

    #[tokio::test]
    async fn store_content_with_newlines_and_tabs() {
        let (_tmp, mem) = temp_sqlite();
        let content = "line1\nline2\ttab\rcarriage\n\nnewparagraph";
        mem.upsert_projection_entry("whitespace", content, MemoryCategory::Core)
            .await
            .unwrap();
        let entry = mem
            .fetch_projection_entry("whitespace")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.content, content);
    }

    // ── Edge cases: recall ───────────────────────────────────────

    #[tokio::test]
    async fn recall_single_character_query() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "x marks the spot", MemoryCategory::Core)
            .await
            .unwrap();
        // Single char may not match FTS5 but LIKE fallback should work
        let results = mem.search_projection("x", 10).await.unwrap();
        // Should not crash; may or may not find results
        assert!(results.len() <= 10);
    }

    #[tokio::test]
    async fn recall_limit_zero() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "some content", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("some", 0).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn recall_limit_one() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("a", "matching content alpha", MemoryCategory::Core)
            .await
            .unwrap();
        mem.upsert_projection_entry("b", "matching content beta", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("matching content", 1).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn recall_matches_by_key_not_just_content() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry(
            "rust_preferences",
            "User likes systems programming",
            MemoryCategory::Core,
        )
        .await
        .unwrap();
        // "rust" appears in key but not content — LIKE fallback checks key too
        let results = mem.search_projection("rust", 10).await.unwrap();
        assert!(!results.is_empty(), "Should match by key");
    }

    #[tokio::test]
    async fn recall_unicode_query() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("jp", "日本語のテスト", MemoryCategory::Core)
            .await
            .unwrap();
        let results = mem.search_projection("日本語", 10).await.unwrap();
        assert!(!results.is_empty());
    }

    // ── Edge cases: schema idempotency ───────────────────────────

    #[tokio::test]
    async fn schema_idempotent_reopen() {
        let tmp = TempDir::new().unwrap();
        {
            let mem = SqliteMemory::new(tmp.path()).unwrap();
            mem.upsert_projection_entry("k1", "v1", MemoryCategory::Core)
                .await
                .unwrap();
        }
        // Open again — init_schema runs again on existing DB
        let mem2 = SqliteMemory::new(tmp.path()).unwrap();
        let entry = mem2.fetch_projection_entry("k1").await.unwrap();
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().content, "v1");
        // Store more data — should work fine
        mem2.upsert_projection_entry("k2", "v2", MemoryCategory::Daily)
            .await
            .unwrap();
        assert_eq!(mem2.count_projection_entries().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn schema_triple_open() {
        let tmp = TempDir::new().unwrap();
        let _m1 = SqliteMemory::new(tmp.path()).unwrap();
        let _m2 = SqliteMemory::new(tmp.path()).unwrap();
        let m3 = SqliteMemory::new(tmp.path()).unwrap();
        assert!(m3.health_check().await);
    }

    // ── Edge cases: forget + FTS5 consistency ────────────────────

    #[tokio::test]
    async fn forget_then_recall_no_ghost_results() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("ghost", "phantom memory content", MemoryCategory::Core)
            .await
            .unwrap();
        mem.delete_projection_entry("ghost").await.unwrap();
        let results = mem.search_projection("phantom memory", 10).await.unwrap();
        assert!(
            results.is_empty(),
            "Deleted memory should not appear in recall"
        );
    }

    #[tokio::test]
    async fn forget_and_re_store_same_key() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("cycle", "version 1", MemoryCategory::Core)
            .await
            .unwrap();
        mem.delete_projection_entry("cycle").await.unwrap();
        mem.upsert_projection_entry("cycle", "version 2", MemoryCategory::Core)
            .await
            .unwrap();
        let entry = mem.fetch_projection_entry("cycle").await.unwrap().unwrap();
        assert_eq!(entry.content, "version 2");
        assert_eq!(mem.count_projection_entries().await.unwrap(), 1);
    }

    // ── Edge cases: reindex ──────────────────────────────────────

    #[tokio::test]
    async fn reindex_empty_db() {
        let (_tmp, mem) = temp_sqlite();
        let count = mem.reindex().await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn reindex_twice_is_safe() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("r1", "reindex data", MemoryCategory::Core)
            .await
            .unwrap();
        mem.reindex().await.unwrap();
        let count = mem.reindex().await.unwrap();
        assert_eq!(count, 0); // Noop embedder → nothing to re-embed
                              // Data should still be intact
        let results = mem.search_projection("reindex", 10).await.unwrap();
        assert_eq!(results.len(), 1);
    }

    // ── Edge cases: content_hash ─────────────────────────────────

    #[test]
    fn content_hash_empty_string() {
        let h = SqliteMemory::content_hash("");
        assert!(!h.is_empty());
        assert_eq!(h.len(), 16); // 16 hex chars
    }

    #[test]
    fn content_hash_unicode() {
        let h1 = SqliteMemory::content_hash("🦀");
        let h2 = SqliteMemory::content_hash("🦀");
        assert_eq!(h1, h2);
        let h3 = SqliteMemory::content_hash("🚀");
        assert_ne!(h1, h3);
    }

    #[test]
    fn content_hash_long_input() {
        let long = "a".repeat(1_000_000);
        let h = SqliteMemory::content_hash(&long);
        assert_eq!(h.len(), 16);
    }

    // ── Edge cases: category helpers ─────────────────────────────

    #[test]
    fn category_roundtrip_custom_with_spaces() {
        let cat = MemoryCategory::Custom("my custom category".into());
        let s = SqliteMemory::category_to_str(&cat);
        assert_eq!(s, "my custom category");
        let back = SqliteMemory::str_to_category(&s);
        assert_eq!(back, cat);
    }

    #[test]
    fn category_roundtrip_empty_custom() {
        let cat = MemoryCategory::Custom(String::new());
        let s = SqliteMemory::category_to_str(&cat);
        assert_eq!(s, "");
        let back = SqliteMemory::str_to_category(&s);
        assert_eq!(back, MemoryCategory::Custom(String::new()));
    }

    // ── Edge cases: list ─────────────────────────────────────────

    #[tokio::test]
    async fn list_custom_category() {
        let (_tmp, mem) = temp_sqlite();
        mem.upsert_projection_entry("c1", "custom1", MemoryCategory::Custom("project".into()))
            .await
            .unwrap();
        mem.upsert_projection_entry("c2", "custom2", MemoryCategory::Custom("project".into()))
            .await
            .unwrap();
        mem.upsert_projection_entry("c3", "other", MemoryCategory::Core)
            .await
            .unwrap();

        let project = mem
            .list_projection_entries(Some(&MemoryCategory::Custom("project".into())))
            .await
            .unwrap();
        assert_eq!(project.len(), 2);
    }

    #[tokio::test]
    async fn list_empty_db() {
        let (_tmp, mem) = temp_sqlite();
        let all = mem.list_projection_entries(None).await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn memory_inferred_claim_persists() {
        let (_tmp, mem) = temp_sqlite();

        let events = mem
            .append_inference_events(vec![MemoryInferenceEvent::inferred_claim(
                "default",
                "persona.preference.language",
                "User prefers Rust",
            )])
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.to_string(), "inferred_claim");

        let slot = mem
            .resolve_slot("default", "persona.preference.language")
            .await
            .unwrap()
            .expect("inferred slot should be available");
        assert_eq!(slot.value, "User prefers Rust");
        assert_eq!(slot.source, MemorySource::Inferred);
    }

    #[tokio::test]
    async fn memory_contradiction_event_recorded() {
        let (_tmp, mem) = temp_sqlite();

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone",
                MemoryEventType::FactAdded,
                "UTC+9",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.95)
            .with_importance(0.8),
        )
        .await
        .unwrap();

        let events = mem
            .append_inference_events(vec![MemoryInferenceEvent::contradiction_marked(
                "default",
                "profile.timezone",
                "Conflict detected: prior=UTC+9 incoming=UTC",
            )])
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.to_string(), "contradiction_marked");

        let slot = mem
            .resolve_slot("default", "profile.timezone")
            .await
            .unwrap()
            .expect("existing explicit slot must remain");
        assert_eq!(slot.value, "UTC+9");
        assert_eq!(slot.source, MemorySource::ExplicitUser);

        let conn = mem.conn.lock().unwrap();
        let contradiction_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_events WHERE event_type = 'contradiction_marked'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(contradiction_count, 1);
    }

    #[tokio::test]
    async fn sqlite_contradiction_penalty_affects_order() {
        let (_tmp, mem) = temp_sqlite();
        let newer = Utc::now();
        let older = newer - Duration::days(2);

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "profile.timezone",
                MemoryEventType::FactAdded,
                "Preferred timezone is UTC for meetings",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.7)
            .with_occurred_at(newer.to_rfc3339()),
        )
        .await
        .unwrap();

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "profile.alt_timezone",
                MemoryEventType::FactAdded,
                "Secondary timezone is UTC for travel",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.7)
            .with_occurred_at(older.to_rfc3339()),
        )
        .await
        .unwrap();

        mem.append_inference_event(
            MemoryInferenceEvent::contradiction_marked(
                "default",
                "profile.timezone",
                "Conflict detected: timezone is not stable",
            )
            .with_confidence(1.0)
            .with_importance(1.0)
            .with_occurred_at((newer + Duration::minutes(1)).to_rfc3339()),
        )
        .await
        .unwrap();

        let recalled = mem
            .recall_scoped(RecallQuery::new("default", "timezone", 10))
            .await
            .unwrap();

        let contradicted_index = recalled
            .iter()
            .position(|item| item.slot_key == "profile.timezone")
            .expect("contradicted slot must be returned");
        let clean_index = recalled
            .iter()
            .position(|item| item.slot_key == "profile.alt_timezone")
            .expect("non-contradicted slot must be returned");

        assert!(
            clean_index < contradicted_index,
            "contradicted slot should rank lower than clean slot"
        );
    }

    #[tokio::test]
    async fn sqlite_trend_ttl_decay_applied() {
        let (_tmp, mem) = temp_sqlite();
        let stale = Utc::now() - Duration::days(120);
        let fresh = Utc::now();

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "trend.productivity.focus",
                MemoryEventType::FactAdded,
                "Focus trend indicates latency spikes during afternoons",
                MemorySource::System,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.7)
            .with_occurred_at(stale.to_rfc3339()),
        )
        .await
        .unwrap();

        mem.append_event(
            MemoryEventInput::new(
                "default",
                "profile.performance_note",
                MemoryEventType::FactAdded,
                "Current note reports latency spikes during afternoons",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_confidence(0.9)
            .with_importance(0.7)
            .with_occurred_at(fresh.to_rfc3339()),
        )
        .await
        .unwrap();

        let recalled = mem
            .recall_scoped(RecallQuery::new("default", "latency", 10))
            .await
            .unwrap();

        let stale_trend = recalled
            .iter()
            .find(|item| item.slot_key == "trend.productivity.focus")
            .expect("stale trend slot must be returned");
        let fresh_note = recalled
            .iter()
            .find(|item| item.slot_key == "profile.performance_note")
            .expect("fresh note slot must be returned");

        assert!(
            stale_trend.score < fresh_note.score,
            "stale trend score should be demoted below fresh note"
        );
    }
}
