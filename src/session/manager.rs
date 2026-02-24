use super::compaction::{self, CompactionConfig, CompactionResult};
use super::store::SessionStore;
use super::types::{ChatMessage, MessageRole, Session, SessionConfig, SessionState};
use anyhow::Result;
use std::sync::Arc;

/// High-level session management wrapping a `SessionStore` + config.
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    config: SessionConfig,
    compaction_config: CompactionConfig,
}

impl SessionManager {
    pub fn new(store: Arc<dyn SessionStore>, config: SessionConfig) -> Self {
        let compaction_config = CompactionConfig {
            threshold: config.compaction_threshold,
            ..CompactionConfig::default()
        };
        Self {
            store,
            config,
            compaction_config,
        }
    }

    pub fn with_compaction_config(mut self, compaction_config: CompactionConfig) -> Self {
        self.compaction_config = compaction_config;
        self
    }

    /// Get an existing active session or create a new one.
    pub async fn get_or_create(&self, channel: &str, user_id: &str) -> Result<Session> {
        self.store.get_or_create_session(channel, user_id).await
    }

    /// Save a turn (user message + assistant response) and optionally compact.
    pub async fn save(
        &self,
        session_id: &str,
        user_message: &str,
        assistant_response: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Result<()> {
        self.store
            .append_message(session_id, MessageRole::User, user_message, None, None)
            .await?;
        self.store
            .append_message(
                session_id,
                MessageRole::Assistant,
                assistant_response,
                input_tokens,
                output_tokens,
            )
            .await?;
        Ok(())
    }

    /// Compact the session if it exceeds the configured threshold.
    pub async fn compact_if_needed(&self, session_id: &str) -> Result<CompactionResult> {
        if self.config.compaction_threshold == 0 {
            return Ok(CompactionResult {
                compacted: false,
                messages_removed: 0,
                level: None,
                summary_injected: false,
            });
        }
        compaction::compact_session(
            self.store.as_ref(),
            session_id,
            None,
            &self.compaction_config,
        )
        .await
    }

    /// Get message history for a session.
    pub async fn get_history(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        let limit = if self.config.max_history > 0 {
            Some(self.config.max_history)
        } else {
            None
        };
        self.store.get_messages(session_id, limit).await
    }

    /// Archive the current active session and create a fresh one.
    pub async fn reset_session(&self, channel: &str, user_id: &str) -> Result<Session> {
        if let Some(existing) = self.find_active_session(channel, user_id).await? {
            self.store
                .update_session_state(&existing.id, SessionState::Archived)
                .await?;
        }
        self.store.create_session(channel, user_id).await
    }

    /// Delete a session by ID.
    pub async fn delete(&self, id: &str) -> Result<bool> {
        self.store.delete_session(id).await
    }

    /// List sessions, optionally filtered by channel.
    pub async fn list(&self, channel: Option<&str>) -> Result<Vec<Session>> {
        self.store.list_sessions(channel).await
    }

    /// Access the underlying store.
    pub fn store(&self) -> &dyn SessionStore {
        self.store.as_ref()
    }

    async fn find_active_session(&self, channel: &str, user_id: &str) -> Result<Option<Session>> {
        let sessions = self.store.list_sessions(Some(channel)).await?;
        Ok(sessions
            .into_iter()
            .find(|session| session.user_id == user_id && session.state == SessionState::Active))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::SqliteSessionStore;
    use crate::session::types::{MessageRole, SessionConfig, SessionState};
    use sqlx::sqlite::SqlitePoolOptions;

    async fn manager() -> SessionManager {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let store = Arc::new(SqliteSessionStore::new(pool).await.unwrap());
        SessionManager::new(store, SessionConfig::default())
    }

    #[tokio::test]
    async fn get_or_create_creates_then_reuses_session() {
        let manager = manager().await;

        let first = manager.get_or_create("cli", "user-1").await.unwrap();
        let second = manager.get_or_create("cli", "user-1").await.unwrap();

        assert_eq!(first.id, second.id);
    }

    #[tokio::test]
    async fn save_appends_user_and_assistant_messages() {
        let manager = manager().await;
        let session = manager.get_or_create("cli", "user-1").await.unwrap();

        manager
            .save(&session.id, "hello", "world", Some(10), Some(20))
            .await
            .unwrap();

        let messages = manager
            .store()
            .get_messages(&session.id, None)
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[1].role, MessageRole::Assistant);
    }

    #[tokio::test]
    async fn get_history_returns_messages() {
        let manager = manager().await;
        let session = manager.get_or_create("cli", "user-1").await.unwrap();
        manager
            .save(&session.id, "hello", "world", None, None)
            .await
            .unwrap();

        let history = manager.get_history(&session.id).await.unwrap();
        assert!(!history.is_empty());
    }

    #[tokio::test]
    async fn reset_session_archives_existing_and_creates_new() {
        let manager = manager().await;
        let first = manager.get_or_create("cli", "user-1").await.unwrap();

        let second = manager.reset_session("cli", "user-1").await.unwrap();
        assert_ne!(first.id, second.id);

        let archived = manager
            .store()
            .get_session(&first.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(archived.state, SessionState::Archived);
        assert_eq!(second.state, SessionState::Active);
    }

    #[tokio::test]
    async fn compact_if_needed_skips_when_disabled() {
        let store: Arc<dyn SessionStore> = {
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            Arc::new(SqliteSessionStore::new(pool).await.unwrap())
        };
        let config = SessionConfig {
            compaction_threshold: 0,
            ..SessionConfig::default()
        };
        let manager = SessionManager::new(store, config);
        let session = manager.get_or_create("cli", "u1").await.unwrap();

        let result = manager.compact_if_needed(&session.id).await.unwrap();
        assert!(!result.compacted);
    }
}
