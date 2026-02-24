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
    let memory = SqliteMemory::new(tmp.path()).await.unwrap();

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
             FROM retrieval_units WHERE unit_id = 'default:persona.preference.language'",
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
