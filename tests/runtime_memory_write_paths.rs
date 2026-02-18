use std::sync::Arc;

use anyhow::Result;
use asteroniris::agent::loop_::run_main_session_turn_for_integration_with_policy;
use asteroniris::channels::build_system_prompt;
use asteroniris::config::Config;
use asteroniris::memory::{Memory, SqliteMemory};
use asteroniris::providers::Provider;
use asteroniris::security::policy::TenantPolicyContext;
use asteroniris::security::SecurityPolicy;
use async_trait::async_trait;
use rusqlite::{params, Connection};
use tempfile::TempDir;

struct FixedResponseProvider {
    response: String,
}

#[async_trait]
impl Provider for FixedResponseProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> Result<String> {
        Ok(self.response.clone())
    }
}

fn event_metadata(conn: &Connection, entity_id: &str, slot_key: &str) -> (String, String, String) {
    conn.query_row(
        "SELECT layer, provenance_source_class, provenance_reference
         FROM memory_events
         WHERE entity_id = ?1 AND slot_key = ?2
         ORDER BY ingested_at DESC
         LIMIT 1",
        params![entity_id, slot_key],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .unwrap()
}

#[tokio::test]
#[allow(clippy::field_reassign_with_default)]
async fn memory_autosave_includes_layer_provenance() {
    let temp = TempDir::new().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();

    let mut config = Config::default();
    config.workspace_dir = workspace.clone();
    config.memory.backend = "sqlite".to_string();
    config.memory.auto_save = true;
    config.persona.enabled_main_session = false;

    let mem: Arc<dyn Memory> = Arc::new(SqliteMemory::new(&workspace).unwrap());
    let provider = FixedResponseProvider {
        response: "INFERRED_CLAIM inference.preference.language => User prefers Rust\nCONTRADICTION_EVENT contradiction.preference.language => Earlier note said Python".to_string(),
    };
    let security = SecurityPolicy::from_config(&config.autonomy, &config.workspace_dir);
    let entity_id = "tenant-alpha:user-42";

    let response = run_main_session_turn_for_integration_with_policy(
        &config,
        &security,
        mem,
        &provider,
        &provider,
        "system",
        "test-model",
        0.3,
        entity_id,
        TenantPolicyContext::enabled("tenant-alpha"),
        "capture autosave metadata",
    )
    .await
    .unwrap();
    assert!(response.contains("INFERRED_CLAIM"));

    let conn = Connection::open(workspace.join("memory").join("brain.db")).unwrap();

    assert_eq!(
        event_metadata(&conn, entity_id, "conversation.user_msg"),
        (
            "working".to_string(),
            "explicit_user".to_string(),
            "agent.autosave.user_msg".to_string()
        )
    );
    assert_eq!(
        event_metadata(&conn, entity_id, "conversation.assistant_resp"),
        (
            "working".to_string(),
            "system".to_string(),
            "agent.autosave.assistant_resp".to_string()
        )
    );
    assert_eq!(
        event_metadata(&conn, entity_id, "inference.preference.language"),
        (
            "semantic".to_string(),
            "inferred".to_string(),
            "inference.post_turn.inferred_claim".to_string()
        )
    );
    assert_eq!(
        event_metadata(&conn, entity_id, "contradiction.preference.language"),
        (
            "episodic".to_string(),
            "system".to_string(),
            "inference.post_turn.contradiction_event".to_string()
        )
    );
}

#[test]
fn prompt_no_daily_memory_injection() {
    let ws = TempDir::new().unwrap();
    std::fs::write(ws.path().join("SOUL.md"), "# Soul\nBe helpful.").unwrap();
    std::fs::write(
        ws.path().join("IDENTITY.md"),
        "# Identity\nName: AsteronIris",
    )
    .unwrap();
    std::fs::write(ws.path().join("USER.md"), "# User\nName: Runtime Test").unwrap();
    std::fs::write(
        ws.path().join("AGENTS.md"),
        "# Agents\nFollow instructions.",
    )
    .unwrap();
    std::fs::write(ws.path().join("TOOLS.md"), "# Tools\nUse tools.").unwrap();
    std::fs::write(ws.path().join("HEARTBEAT.md"), "# Heartbeat\nStable.").unwrap();
    std::fs::write(ws.path().join("MEMORY.md"), "# Memory\nCurated memory.").unwrap();

    let memory_dir = ws.path().join("memory");
    std::fs::create_dir_all(&memory_dir).unwrap();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    std::fs::write(
        memory_dir.join(format!("{today}.md")),
        "# Daily\nSome note.",
    )
    .unwrap();

    let prompt = build_system_prompt(ws.path(), "model", &[], &[]);
    assert!(!prompt.contains("Daily Notes"));
    assert!(!prompt.contains("Some note"));
}
