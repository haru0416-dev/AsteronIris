use super::memory_harness::sqlite_fixture as temp_sqlite;
use rusqlite::Connection;

#[tokio::test]
async fn sqlite_schema_contains_core_tables() {
    let (tmp, _mem) = temp_sqlite().await;
    let db_path = tmp.path().join("memory").join("brain.db");
    let conn = Connection::open(db_path).expect("open db");

    let retrieval_units_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='retrieval_units'",
            [],
            |row| row.get(0),
        )
        .expect("query retrieval_units table");
    let events_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memory_events'",
            [],
            |row| row.get(0),
        )
        .expect("query events table");

    assert_eq!(retrieval_units_count, 1);
    assert_eq!(events_count, 1);
}

#[tokio::test]
async fn sqlite_schema_contains_fts_table() {
    let (tmp, _mem) = temp_sqlite().await;
    let db_path = tmp.path().join("memory").join("brain.db");
    let conn = Connection::open(db_path).expect("open db");

    let fts_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='retrieval_fts'",
            [],
            |row| row.get(0),
        )
        .expect("query fts table");

    assert_eq!(fts_count, 1);
}
