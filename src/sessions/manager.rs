use super::compaction;
use super::store::{SessionStore, SqliteSessionStore};
use super::types::{ChatMessage, MessageRole, Session, SessionConfig, SessionState};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub struct SessionManager {
    store: Arc<SqliteSessionStore>,
    config: SessionConfig,
}

impl SessionManager {
    pub fn new(db_path: &Path, config: SessionConfig) -> Result<Self> {
        let store = Arc::new(SqliteSessionStore::new(db_path)?);
        Ok(Self { store, config })
    }

    pub fn resolve_session(&self, channel: &str, user_id: &str) -> Result<Session> {
        self.store.get_or_create_session(channel, user_id)
    }

    pub fn record_turn(
        &self,
        session_id: &str,
        user_message: &str,
        assistant_response: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Result<()> {
        self.store
            .append_message(session_id, MessageRole::User, user_message, None, None)?;
        self.store.append_message(
            session_id,
            MessageRole::Assistant,
            assistant_response,
            input_tokens,
            output_tokens,
        )?;

        if self.config.compaction_threshold > 0 {
            let _ = compaction::compact_session(
                self.store.as_ref(),
                session_id,
                self.config.compaction_threshold,
            );
        }

        Ok(())
    }

    pub fn get_history(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let limit = if self.config.max_history > 0 {
            Some(self.config.max_history)
        } else {
            None
        };
        self.store.get_messages(session_id, limit)
    }

    pub fn reset_session(&self, channel: &str, user_id: &str) -> Result<Session> {
        if let Some(existing) = self.find_active_session(channel, user_id)? {
            self.store
                .update_session_state(&existing.id, SessionState::Archived)?;
        }

        self.store.create_session(channel, user_id)
    }

    pub fn list_sessions(&self, channel: Option<&str>) -> Result<Vec<Session>> {
        self.store.list_sessions(channel)
    }

    pub fn delete_session(&self, id: &str) -> Result<bool> {
        self.store.delete_session(id)
    }

    fn find_active_session(&self, channel: &str, user_id: &str) -> Result<Option<Session>> {
        let sessions = self.store.list_sessions(Some(channel))?;
        Ok(sessions
            .into_iter()
            .find(|session| session.user_id == user_id && session.state == SessionState::Active))
    }

    pub fn store(&self) -> &SqliteSessionStore {
        self.store.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::SessionManager;
    use crate::sessions::store::SessionStore;
    use crate::sessions::types::{MessageRole, SessionConfig, SessionState};
    use tempfile::NamedTempFile;

    fn manager() -> (NamedTempFile, SessionManager) {
        let db_file = NamedTempFile::new().unwrap();
        let manager = SessionManager::new(db_file.path(), SessionConfig::default()).unwrap();
        (db_file, manager)
    }

    #[test]
    fn resolve_session_creates_then_reuses_session() {
        let (_db_file, manager) = manager();

        let first = manager.resolve_session("cli", "user-1").unwrap();
        let second = manager.resolve_session("cli", "user-1").unwrap();

        assert_eq!(first.id, second.id);
    }

    #[test]
    fn record_turn_appends_user_and_assistant_messages() {
        let (_db_file, manager) = manager();
        let session = manager.resolve_session("cli", "user-1").unwrap();

        manager
            .record_turn(&session.id, "hello", "world", Some(10), Some(20))
            .unwrap();

        let messages = manager.store().get_messages(&session.id, None).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[1].role, MessageRole::Assistant);
    }

    #[test]
    fn get_history_returns_messages() {
        let (_db_file, manager) = manager();
        let session = manager.resolve_session("cli", "user-1").unwrap();
        manager
            .record_turn(&session.id, "hello", "world", None, None)
            .unwrap();

        let history = manager.get_history(&session.id).unwrap();
        assert!(!history.is_empty());
    }

    #[test]
    fn reset_session_archives_existing_and_creates_new() {
        let (_db_file, manager) = manager();
        let first = manager.resolve_session("cli", "user-1").unwrap();

        let second = manager.reset_session("cli", "user-1").unwrap();
        assert_ne!(first.id, second.id);

        let archived = manager.store().get_session(&first.id).unwrap().unwrap();
        assert_eq!(archived.state, SessionState::Archived);
        assert_eq!(second.state, SessionState::Active);
    }
}
