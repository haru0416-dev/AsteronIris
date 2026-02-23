use super::SqliteMemory;
use anyhow::Context;
use rusqlite::Connection;
impl SqliteMemory {
    #[allow(clippy::too_many_lines)]
    pub(super) fn init_schema(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "-- Embedding cache with LRU eviction
            CREATE TABLE IF NOT EXISTS embedding_cache (
                content_hash TEXT PRIMARY KEY,
                embedding    BLOB NOT NULL,
                created_at   TEXT NOT NULL,
                accessed_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cache_accessed ON embedding_cache(accessed_at);",
        )
        .context("initialize core memory schema")?;
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
                signal_tier TEXT NOT NULL DEFAULT 'raw',
                source_kind TEXT,
                privacy_level TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                ingested_at TEXT NOT NULL,
                supersedes_event_id TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_memory_events_entity_slot
                ON memory_events(entity_id, slot_key, occurred_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_events_entity_layer
                ON memory_events(entity_id, layer, occurred_at DESC);
            CREATE INDEX IF NOT EXISTS idx_memory_events_retention_expires
                ON memory_events(retention_expires_at)
                WHERE retention_expires_at IS NOT NULL;
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
            CREATE TABLE IF NOT EXISTS retrieval_units (
                unit_id             TEXT PRIMARY KEY,
                entity_id           TEXT NOT NULL,
                slot_key            TEXT NOT NULL,
                content             TEXT NOT NULL,
                content_type        TEXT NOT NULL DEFAULT 'belief',
                signal_tier         TEXT NOT NULL DEFAULT 'belief',
                promotion_status    TEXT NOT NULL DEFAULT 'promoted',
                chunk_index         INTEGER,
                source_uri          TEXT,
                source_kind         TEXT,
                recency_score       REAL NOT NULL DEFAULT 1.0,
                importance          REAL NOT NULL DEFAULT 0.5,
                reliability         REAL NOT NULL DEFAULT 0.8,
                contradiction_penalty REAL NOT NULL DEFAULT 0.0,
                visibility          TEXT NOT NULL DEFAULT 'public',

                embedding           BLOB,
                embedding_model     TEXT,
                embedding_dim       INTEGER,
                layer               TEXT NOT NULL DEFAULT 'working',
                provenance_source_class TEXT,
                provenance_reference    TEXT,
                provenance_evidence_uri TEXT,
                retention_tier      TEXT NOT NULL DEFAULT 'working',
                retention_expires_at TEXT,

                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL,
                UNIQUE(entity_id, slot_key, chunk_index)
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS retrieval_fts USING fts5(
                slot_key, content, content=retrieval_units, content_rowid=rowid, tokenize='trigram'
            );
            CREATE TRIGGER IF NOT EXISTS retrieval_units_ai AFTER INSERT ON retrieval_units BEGIN
                INSERT INTO retrieval_fts(rowid, slot_key, content)
                VALUES (new.rowid, new.slot_key, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS retrieval_units_ad AFTER DELETE ON retrieval_units BEGIN
                INSERT INTO retrieval_fts(retrieval_fts, rowid, slot_key, content)
                VALUES ('delete', old.rowid, old.slot_key, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS retrieval_units_au AFTER UPDATE ON retrieval_units BEGIN
                INSERT INTO retrieval_fts(retrieval_fts, rowid, slot_key, content)
                VALUES ('delete', old.rowid, old.slot_key, old.content);
                INSERT INTO retrieval_fts(rowid, slot_key, content)
                VALUES (new.rowid, new.slot_key, new.content);
            END;
            CREATE INDEX IF NOT EXISTS idx_retrieval_units_entity ON retrieval_units(entity_id);
            CREATE INDEX IF NOT EXISTS idx_retrieval_units_entity_slot ON retrieval_units(entity_id, slot_key);
            CREATE INDEX IF NOT EXISTS idx_retrieval_units_signal_tier ON retrieval_units(signal_tier);
            CREATE INDEX IF NOT EXISTS idx_retrieval_units_promotion ON retrieval_units(promotion_status);
            CREATE INDEX IF NOT EXISTS idx_retrieval_units_entity_visibility ON retrieval_units(entity_id, visibility, updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_retrieval_units_retention ON retrieval_units(retention_expires_at) WHERE retention_expires_at IS NOT NULL;

            CREATE TABLE IF NOT EXISTS deletion_ledger (
                ledger_id TEXT PRIMARY KEY,
                entity_id TEXT NOT NULL,
                target_slot_key TEXT NOT NULL,
                phase TEXT NOT NULL,
                reason TEXT NOT NULL,
                requested_by TEXT NOT NULL,
                executed_at TEXT NOT NULL
            );",
        )
        .context("initialize event schema tables")?;
        Ok(())
    }
    #[cfg(test)]
    fn table_exists(conn: &Connection, table_name: &str) -> anyhow::Result<bool> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                rusqlite::params![table_name],
                |row| row.get(0),
            )
            .context("check schema table existence")?;
        Ok(count == 1)
    }
    #[cfg(test)]
    fn table_columns(conn: &Connection, table_name: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .context("prepare table info query")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .context("query table column info")?;
        let mut columns = Vec::new();
        for row in rows {
            columns.push(row.context("read column info row")?);
        }
        Ok(columns)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        SqliteMemory::init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn init_schema_creates_expected_tables() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteMemory::init_schema(&conn).unwrap();
        let expected_tables = [
            "memory_events",
            "belief_slots",
            "retrieval_units",
            "retrieval_fts",
            "deletion_ledger",
            "embedding_cache",
        ];
        for table in expected_tables {
            assert!(SqliteMemory::table_exists(&conn, table).unwrap());
        }
    }

    #[test]
    fn init_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        SqliteMemory::init_schema(&conn).unwrap();
        SqliteMemory::init_schema(&conn).unwrap();
    }

    #[test]
    fn table_exists_reports_true_and_false() {
        let conn = fresh_db();
        assert!(SqliteMemory::table_exists(&conn, "retrieval_units").unwrap());
        assert!(!SqliteMemory::table_exists(&conn, "not_a_real_table").unwrap());
    }

    #[test]
    fn table_columns_returns_expected_columns() {
        let conn = fresh_db();
        let columns = SqliteMemory::table_columns(&conn, "memory_events").unwrap();
        assert!(columns.contains(&"event_id".to_string()));
        assert!(columns.contains(&"entity_id".to_string()));
        assert!(columns.contains(&"layer".to_string()));
    }
}
