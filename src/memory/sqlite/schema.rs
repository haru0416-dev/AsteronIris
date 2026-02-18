use super::SqliteMemory;
use chrono::Local;
use rusqlite::{params, Connection};

impl SqliteMemory {
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
}
