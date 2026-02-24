use anyhow::Context;
use sqlx::SqlitePool;

/// DDL for the embedding cache table.
const CREATE_EMBEDDING_CACHE: &str = "
CREATE TABLE IF NOT EXISTS embedding_cache (
    content_hash TEXT PRIMARY KEY,
    embedding    BLOB NOT NULL,
    created_at   TEXT NOT NULL,
    accessed_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cache_accessed ON embedding_cache(accessed_at);
";

/// DDL for event-sourcing and materialised-view tables.
const CREATE_EVENT_TABLES: &str = "
CREATE TABLE IF NOT EXISTS memory_events (
    event_id                TEXT PRIMARY KEY,
    entity_id               TEXT NOT NULL,
    slot_key                TEXT NOT NULL,
    layer                   TEXT NOT NULL DEFAULT 'working',
    event_type              TEXT NOT NULL,
    value                   TEXT NOT NULL,
    source                  TEXT NOT NULL,
    confidence              REAL NOT NULL,
    importance              REAL NOT NULL,
    provenance_source_class TEXT,
    provenance_reference    TEXT,
    provenance_evidence_uri TEXT,
    retention_tier          TEXT NOT NULL DEFAULT 'working',
    retention_expires_at    TEXT,
    signal_tier             TEXT NOT NULL DEFAULT 'raw',
    source_kind             TEXT,
    privacy_level           TEXT NOT NULL,
    occurred_at             TEXT NOT NULL,
    ingested_at             TEXT NOT NULL,
    supersedes_event_id     TEXT
);
CREATE INDEX IF NOT EXISTS idx_memory_events_entity_slot
    ON memory_events(entity_id, slot_key, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_events_entity_layer
    ON memory_events(entity_id, layer, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_events_retention_expires
    ON memory_events(retention_expires_at)
    WHERE retention_expires_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS belief_slots (
    entity_id        TEXT NOT NULL,
    slot_key         TEXT NOT NULL,
    value            TEXT NOT NULL,
    status           TEXT NOT NULL,
    winner_event_id  TEXT NOT NULL,
    source           TEXT NOT NULL,
    confidence       REAL NOT NULL,
    importance       REAL NOT NULL,
    privacy_level    TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    PRIMARY KEY(entity_id, slot_key)
);

CREATE TABLE IF NOT EXISTS retrieval_units (
    unit_id                 TEXT PRIMARY KEY,
    entity_id               TEXT NOT NULL,
    slot_key                TEXT NOT NULL,
    content                 TEXT NOT NULL,
    content_type            TEXT NOT NULL DEFAULT 'belief',
    signal_tier             TEXT NOT NULL DEFAULT 'belief',
    promotion_status        TEXT NOT NULL DEFAULT 'promoted',
    chunk_index             INTEGER,
    source_uri              TEXT,
    source_kind             TEXT,
    recency_score           REAL NOT NULL DEFAULT 1.0,
    importance              REAL NOT NULL DEFAULT 0.5,
    reliability             REAL NOT NULL DEFAULT 0.8,
    contradiction_penalty   REAL NOT NULL DEFAULT 0.0,
    visibility              TEXT NOT NULL DEFAULT 'public',
    embedding               BLOB,
    embedding_model         TEXT,
    embedding_dim           INTEGER,
    layer                   TEXT NOT NULL DEFAULT 'working',
    provenance_source_class TEXT,
    provenance_reference    TEXT,
    provenance_evidence_uri TEXT,
    retention_tier          TEXT NOT NULL DEFAULT 'working',
    retention_expires_at    TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    UNIQUE(entity_id, slot_key, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_retrieval_units_entity
    ON retrieval_units(entity_id);
CREATE INDEX IF NOT EXISTS idx_retrieval_units_entity_slot
    ON retrieval_units(entity_id, slot_key);
CREATE INDEX IF NOT EXISTS idx_retrieval_units_signal_tier
    ON retrieval_units(signal_tier);
CREATE INDEX IF NOT EXISTS idx_retrieval_units_promotion
    ON retrieval_units(promotion_status);
CREATE INDEX IF NOT EXISTS idx_retrieval_units_entity_visibility
    ON retrieval_units(entity_id, visibility, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_retrieval_units_retention
    ON retrieval_units(retention_expires_at)
    WHERE retention_expires_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS associations (
    source_id   TEXT NOT NULL,
    target_id   TEXT NOT NULL,
    kind        TEXT NOT NULL,
    confidence  REAL NOT NULL DEFAULT 1.0,
    created_at  TEXT NOT NULL,
    PRIMARY KEY(source_id, target_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_associations_source ON associations(source_id);
CREATE INDEX IF NOT EXISTS idx_associations_target ON associations(target_id);

CREATE TABLE IF NOT EXISTS deletion_ledger (
    ledger_id        TEXT PRIMARY KEY,
    entity_id        TEXT NOT NULL,
    target_slot_key  TEXT NOT NULL,
    phase            TEXT NOT NULL,
    reason           TEXT NOT NULL,
    requested_by     TEXT NOT NULL,
    executed_at      TEXT NOT NULL
);
";

/// DDL for FTS5 virtual table and sync triggers.
///
/// Executed separately because `sqlx::query` cannot batch virtual-table DDL
/// together with regular DDL in the same `execute_batch` call without issues.
const CREATE_FTS: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS retrieval_fts USING fts5(
    slot_key, content, content=retrieval_units, content_rowid=rowid, tokenize='trigram'
);
";

const CREATE_FTS_TRIGGERS: &str = "
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
";

/// `SQLite` connection pragmas for optimal WAL performance.
const PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -8000;
PRAGMA mmap_size = 268435456;
PRAGMA busy_timeout = 5000;
";

/// Initialise the full schema on the given pool.
///
/// All statements use `IF NOT EXISTS` so the function is idempotent.
pub(super) async fn init_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    // Pragmas must run outside a transaction for WAL mode to take effect.
    sqlx::raw_sql(PRAGMAS)
        .execute(pool)
        .await
        .context("configure SQLite pragmas")?;

    sqlx::raw_sql(CREATE_EMBEDDING_CACHE)
        .execute(pool)
        .await
        .context("create embedding_cache table")?;

    sqlx::raw_sql(CREATE_EVENT_TABLES)
        .execute(pool)
        .await
        .context("create event/materialised-view tables")?;

    sqlx::raw_sql(CREATE_FTS)
        .execute(pool)
        .await
        .context("create FTS5 virtual table")?;

    sqlx::raw_sql(CREATE_FTS_TRIGGERS)
        .execute(pool)
        .await
        .context("create FTS5 sync triggers")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("open in-memory SQLite");
        init_schema(&pool).await.expect("init schema");
        pool
    }

    #[tokio::test]
    async fn init_schema_creates_expected_tables() {
        let pool = fresh_pool().await;

        let expected = [
            "memory_events",
            "belief_slots",
            "retrieval_units",
            "retrieval_fts",
            "deletion_ledger",
            "embedding_cache",
            "associations",
        ];
        for table in expected {
            let count: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(count.0, 1, "table {table} should exist");
        }
    }

    #[tokio::test]
    async fn init_schema_is_idempotent() {
        let pool = fresh_pool().await;
        // Second call must not fail.
        init_schema(&pool).await.unwrap();
    }
}
