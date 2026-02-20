use std::fs;

use asteroniris::memory::traits::MemoryLayer;
use asteroniris::memory::{
    Memory, MemoryEventInput, MemoryEventType, MemoryProvenance, MemorySource, PrivacyLevel,
    SqliteMemory,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

#[tokio::test]
async fn sqlite_persists_layer_and_provenance() {
    let tmp = TempDir::new().unwrap();
    let memory = SqliteMemory::new(tmp.path()).unwrap();

    let persisted = memory
        .append_event(
            MemoryEventInput::new(
                "default",
                "persona.preference.language",
                MemoryEventType::FactAdded,
                "Rust",
                MemorySource::ExplicitUser,
                PrivacyLevel::Private,
            )
            .with_layer(MemoryLayer::Episodic)
            .with_provenance(
                MemoryProvenance::source_reference(MemorySource::ExplicitUser, "user:input")
                    .with_evidence_uri("https://example.com/chat/1"),
            ),
        )
        .await
        .unwrap();

    let conn = Connection::open(tmp.path().join("memory").join("brain.db")).unwrap();
    let event_row: (String, Option<String>, Option<String>, Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT layer, provenance_source_class, provenance_reference, provenance_evidence_uri, retention_tier, retention_expires_at
             FROM memory_events WHERE event_id = ?1",
            params![persisted.event_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(event_row.0, "episodic");
    assert_eq!(event_row.1.as_deref(), Some("explicit_user"));
    assert_eq!(event_row.2.as_deref(), Some("user:input"));
    assert_eq!(event_row.3.as_deref(), Some("https://example.com/chat/1"));
    assert_eq!(event_row.4, "episodic");
    assert!(
        event_row.5.is_some(),
        "episodic rows should carry retention expiry"
    );

    let doc_row: (String, Option<String>, Option<String>, Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT layer, provenance_source_class, provenance_reference, provenance_evidence_uri, retention_tier, retention_expires_at
             FROM retrieval_docs WHERE doc_id = 'default:persona.preference.language'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(doc_row.0, "episodic");
    assert_eq!(doc_row.1.as_deref(), Some("explicit_user"));
    assert_eq!(doc_row.2.as_deref(), Some("user:input"));
    assert_eq!(doc_row.3.as_deref(), Some("https://example.com/chat/1"));
    assert_eq!(doc_row.4, "episodic");
    assert!(
        doc_row.5.is_some(),
        "episodic docs should carry retention expiry"
    );
}

#[tokio::test]
async fn sqlite_legacy_rows_still_resolve() {
    let tmp = TempDir::new().unwrap();
    seed_legacy_v2_db(&tmp);

    let memory = SqliteMemory::new(tmp.path()).unwrap();
    let slot = memory
        .resolve_slot("legacy-tenant", "legacy.slot")
        .await
        .unwrap()
        .expect("legacy slot should still resolve");
    assert_eq!(slot.value, "legacy value");
    assert_eq!(slot.source, MemorySource::ExplicitUser);

    assert_eq!(memory.count_events(Some("legacy-tenant")).await.unwrap(), 1);
    assert_eq!(memory.count_events(None).await.unwrap(), 1);

    let conn = Connection::open(tmp.path().join("memory").join("brain.db")).unwrap();
    let schema_version: i64 = conn
        .query_row(
            "SELECT version FROM memory_schema_version WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_version, 3);
}

fn seed_legacy_v2_db(tmp: &TempDir) {
    let db_path = tmp.path().join("memory").join("brain.db");
    fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    let conn = Connection::open(db_path).unwrap();
    conn.execute_batch(
        "PRAGMA user_version = 2;
         CREATE TABLE memories (
             id TEXT PRIMARY KEY,
             key TEXT NOT NULL UNIQUE,
             content TEXT NOT NULL,
             category TEXT NOT NULL DEFAULT 'core',
             embedding BLOB,
             created_at TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE memory_events (
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
             privacy_level TEXT NOT NULL,
             occurred_at TEXT NOT NULL,
             ingested_at TEXT NOT NULL,
             supersedes_event_id TEXT
         );
         CREATE TABLE belief_slots (
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
         CREATE TABLE retrieval_docs (
             doc_id TEXT PRIMARY KEY,
             entity_id TEXT NOT NULL,
             slot_key TEXT NOT NULL,
             text_body TEXT NOT NULL,
             recency_score REAL NOT NULL,
             importance REAL NOT NULL,
             reliability REAL NOT NULL,
             contradiction_penalty REAL NOT NULL DEFAULT 0,
             visibility TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE deletion_ledger (
             ledger_id TEXT PRIMARY KEY,
             entity_id TEXT NOT NULL,
             target_slot_key TEXT NOT NULL,
             phase TEXT NOT NULL,
             reason TEXT NOT NULL,
             requested_by TEXT NOT NULL,
             executed_at TEXT NOT NULL
         );
         CREATE TABLE memory_schema_version (
             id INTEGER PRIMARY KEY CHECK(id = 1),
             version INTEGER NOT NULL,
             updated_at TEXT NOT NULL
         );
         INSERT INTO memory_schema_version (id, version, updated_at)
         VALUES (1, 2, '2024-01-01T00:00:00Z');
         INSERT INTO memories (id, key, content, category, embedding, created_at, updated_at)
         VALUES ('m1', 'legacy.slot', 'legacy value', 'core', NULL, '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z');
         INSERT INTO memory_events (
             event_id, entity_id, slot_key, layer, event_type, value, source,
             confidence, importance, provenance_source_class, provenance_reference, provenance_evidence_uri,
             privacy_level, occurred_at, ingested_at, supersedes_event_id
         ) VALUES (
             'e1', 'legacy-tenant', 'legacy.slot', 'working', 'fact_added', 'legacy value', 'explicit_user',
             0.95, 0.7, NULL, NULL, NULL, 'private', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z', NULL
         );
         INSERT INTO belief_slots (
             entity_id, slot_key, value, status, winner_event_id, source,
             confidence, importance, privacy_level, updated_at
         ) VALUES (
             'legacy-tenant', 'legacy.slot', 'legacy value', 'active', 'e1', 'explicit_user',
             0.95, 0.7, 'private', '2024-01-01T00:00:00Z'
         );",
    )
    .unwrap();
}
