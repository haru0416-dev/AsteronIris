use super::types::{ChatMessage, MessageRole, Session, SessionState};
use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, Error as SqlError, OptionalExtension, params, types::Type};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub trait SessionStore {
    fn create_session(&self, channel: &str, user_id: &str) -> Result<Session>;
    fn get_session(&self, id: &str) -> Result<Option<Session>>;
    fn get_or_create_session(&self, channel: &str, user_id: &str) -> Result<Session>;
    fn list_sessions(&self, channel: Option<&str>) -> Result<Vec<Session>>;
    fn delete_session(&self, id: &str) -> Result<bool>;
    fn update_session_state(&self, id: &str, state: SessionState) -> Result<()>;

    fn append_message(
        &self,
        session_id: &str,
        role: MessageRole,
        content: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Result<ChatMessage>;
    fn get_messages(&self, session_id: &str, limit: Option<usize>) -> Result<Vec<ChatMessage>>;
    fn count_messages(&self, session_id: &str) -> Result<usize>;
    fn delete_messages_before(&self, session_id: &str, before_id: &str) -> Result<usize>;
}

pub struct SqliteSessionStore {
    conn: Mutex<Connection>,
}

impl SqliteSessionStore {
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 channel TEXT NOT NULL,
                 user_id TEXT NOT NULL,
                 state TEXT NOT NULL DEFAULT 'active',
                 model TEXT,
                 metadata TEXT,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 UNIQUE(channel, user_id, state)
             );

             CREATE TABLE IF NOT EXISTS chat_messages (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 input_tokens INTEGER,
                 output_tokens INTEGER,
                 created_at TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_chat_messages_session
                 ON chat_messages(session_id, created_at);",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|error| anyhow::anyhow!("Lock error: {error}"))
    }

    fn state_to_str(state: SessionState) -> &'static str {
        match state {
            SessionState::Active => "active",
            SessionState::Archived => "archived",
            SessionState::Compacted => "compacted",
        }
    }

    fn str_to_state(value: &str, column_index: usize) -> rusqlite::Result<SessionState> {
        match value {
            "active" => Ok(SessionState::Active),
            "archived" => Ok(SessionState::Archived),
            "compacted" => Ok(SessionState::Compacted),
            _ => Err(SqlError::FromSqlConversionFailure(
                column_index,
                Type::Text,
                format!("unknown session state: {value}").into(),
            )),
        }
    }

    fn role_to_str(role: MessageRole) -> &'static str {
        match role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        }
    }

    fn str_to_role(value: &str, column_index: usize) -> rusqlite::Result<MessageRole> {
        match value {
            "user" => Ok(MessageRole::User),
            "assistant" => Ok(MessageRole::Assistant),
            "system" => Ok(MessageRole::System),
            _ => Err(SqlError::FromSqlConversionFailure(
                column_index,
                Type::Text,
                format!("unknown message role: {value}").into(),
            )),
        }
    }

    fn map_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
        let state_raw: String = row.get(3)?;
        let metadata_raw: Option<String> = row.get(5)?;
        let metadata = metadata_raw
            .map(|value| {
                serde_json::from_str::<serde_json::Value>(&value).map_err(|error| {
                    SqlError::FromSqlConversionFailure(5, Type::Text, Box::new(error))
                })
            })
            .transpose()?;

        Ok(Session {
            id: row.get(0)?,
            channel: row.get(1)?,
            user_id: row.get(2)?,
            state: Self::str_to_state(&state_raw, 3)?,
            model: row.get(4)?,
            metadata,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    }

    fn map_chat_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatMessage> {
        let role_raw: String = row.get(2)?;
        Ok(ChatMessage {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: Self::str_to_role(&role_raw, 2)?,
            content: row.get(3)?,
            input_tokens: row
                .get::<_, Option<i64>>(4)?
                .map(|value| i64_to_u64(value, 4))
                .transpose()?,
            output_tokens: row
                .get::<_, Option<i64>>(5)?
                .map(|value| i64_to_u64(value, 5))
                .transpose()?,
            created_at: row.get(6)?,
        })
    }
}

impl SessionStore for SqliteSessionStore {
    fn create_session(&self, channel: &str, user_id: &str) -> Result<Session> {
        let conn = self.lock_connection()?;
        let session_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO sessions (id, channel, user_id, state, model, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?5)",
            params![
                session_id,
                channel,
                user_id,
                Self::state_to_str(SessionState::Active),
                timestamp
            ],
        )?;

        Ok(Session {
            id: session_id,
            channel: channel.to_string(),
            user_id: user_id.to_string(),
            state: SessionState::Active,
            model: None,
            metadata: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        })
    }

    fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.lock_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
             FROM sessions
             WHERE id = ?1",
        )?;

        stmt.query_row(params![id], Self::map_session_row)
            .optional()
            .map_err(Into::into)
    }

    fn get_or_create_session(&self, channel: &str, user_id: &str) -> Result<Session> {
        let conn = self.lock_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
             FROM sessions
             WHERE channel = ?1 AND user_id = ?2 AND state = 'active'
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;

        if let Ok(existing) = stmt.query_row(params![channel, user_id], Self::map_session_row) {
            return Ok(existing);
        }
        drop(stmt);
        drop(conn);

        self.create_session(channel, user_id)
    }

    fn list_sessions(&self, channel: Option<&str>) -> Result<Vec<Session>> {
        let conn = self.lock_connection()?;
        let mut sessions = Vec::new();
        let mut stmt = if channel.is_some() {
            conn.prepare(
                "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
                 FROM sessions
                 WHERE channel = ?1
                 ORDER BY updated_at DESC",
            )?
        } else {
            conn.prepare(
                "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
                 FROM sessions
                 ORDER BY updated_at DESC",
            )?
        };

        if let Some(channel_name) = channel {
            let rows = stmt.query_map(params![channel_name], Self::map_session_row)?;
            for row in rows {
                sessions.push(row?);
            }
        } else {
            let rows = stmt.query_map([], Self::map_session_row)?;
            for row in rows {
                sessions.push(row?);
            }
        }

        Ok(sessions)
    }

    fn delete_session(&self, id: &str) -> Result<bool> {
        let conn = self.lock_connection()?;
        let deleted = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    fn update_session_state(&self, id: &str, state: SessionState) -> Result<()> {
        let conn = self.lock_connection()?;
        let timestamp = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE sessions
             SET state = ?1, updated_at = ?2
             WHERE id = ?3",
            params![Self::state_to_str(state), timestamp, id],
        )?;
        Ok(())
    }

    fn append_message(
        &self,
        session_id: &str,
        role: MessageRole,
        content: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Result<ChatMessage> {
        let conn = self.lock_connection()?;
        let message_id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let input_tokens_i64 = input_tokens.map(i64::try_from).transpose()?;
        let output_tokens_i64 = output_tokens.map(i64::try_from).transpose()?;

        conn.execute(
            "INSERT INTO chat_messages (id, session_id, role, content, input_tokens, output_tokens, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                message_id,
                session_id,
                Self::role_to_str(role),
                content,
                input_tokens_i64,
                output_tokens_i64,
                created_at
            ],
        )?;

        conn.execute(
            "UPDATE sessions
             SET updated_at = ?1
             WHERE id = ?2",
            params![created_at, session_id],
        )?;

        Ok(ChatMessage {
            id: message_id,
            session_id: session_id.to_string(),
            role,
            content: content.to_string(),
            input_tokens,
            output_tokens,
            created_at,
        })
    }

    fn get_messages(&self, session_id: &str, limit: Option<usize>) -> Result<Vec<ChatMessage>> {
        let conn = self.lock_connection()?;

        let mut messages = Vec::new();
        if let Some(limit_count) = limit {
            let limit_i64 = i64::try_from(limit_count)?;
            let mut stmt = conn.prepare(
                "SELECT id, session_id, role, content, input_tokens, output_tokens, created_at
                 FROM chat_messages
                 WHERE session_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )?;
            let rows =
                stmt.query_map(params![session_id, limit_i64], Self::map_chat_message_row)?;
            for row in rows {
                messages.push(row?);
            }
            messages.reverse();
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, role, content, input_tokens, output_tokens, created_at
                 FROM chat_messages
                 WHERE session_id = ?1
                 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![session_id], Self::map_chat_message_row)?;
            for row in rows {
                messages.push(row?);
            }
        }

        Ok(messages)
    }

    fn count_messages(&self, session_id: &str) -> Result<usize> {
        let conn = self.lock_connection()?;
        let count = conn.query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1",
            params![session_id],
            |row| row.get::<_, i64>(0),
        )?;
        usize::try_from(count).map_err(Into::into)
    }

    fn delete_messages_before(&self, session_id: &str, before_id: &str) -> Result<usize> {
        let conn = self.lock_connection()?;
        let deleted = conn.execute(
            "DELETE FROM chat_messages
             WHERE session_id = ?1
               AND created_at <= (
                   SELECT created_at
                   FROM chat_messages
                   WHERE id = ?2 AND session_id = ?1
               )",
            params![session_id, before_id],
        )?;
        Ok(deleted)
    }
}

fn i64_to_u64(value: i64, column_index: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        SqlError::FromSqlConversionFailure(column_index, Type::Integer, Box::new(error))
    })
}

#[cfg(test)]
mod tests {
    use super::{SessionStore, SqliteSessionStore};
    use crate::sessions::types::{MessageRole, SessionState};
    use tempfile::NamedTempFile;

    fn store() -> (NamedTempFile, SqliteSessionStore) {
        let db_file = NamedTempFile::new().unwrap();
        let store = SqliteSessionStore::new(db_file.path()).unwrap();
        (db_file, store)
    }

    #[test]
    fn create_session_returns_valid_session() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "user-1").unwrap();

        assert!(!session.id.is_empty());
        assert_eq!(session.channel, "cli");
        assert_eq!(session.user_id, "user-1");
        assert_eq!(session.state, SessionState::Active);
    }

    #[test]
    fn get_session_finds_existing_and_none_for_missing() {
        let (_db_file, store) = store();
        let created = store.create_session("cli", "user-1").unwrap();

        let found = store.get_session(&created.id).unwrap();
        let missing = store.get_session("missing-id").unwrap();

        assert!(found.is_some());
        assert!(missing.is_none());
    }

    #[test]
    fn get_or_create_session_returns_existing_active_session() {
        let (_db_file, store) = store();
        let first = store.get_or_create_session("discord", "user-99").unwrap();
        let second = store.get_or_create_session("discord", "user-99").unwrap();

        assert_eq!(first.id, second.id);
    }

    #[test]
    fn list_sessions_returns_all_and_filters_by_channel() {
        let (_db_file, store) = store();
        store.create_session("cli", "u1").unwrap();
        store.create_session("discord", "u2").unwrap();

        let all = store.list_sessions(None).unwrap();
        let cli_only = store.list_sessions(Some("cli")).unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(cli_only.len(), 1);
        assert_eq!(cli_only[0].channel, "cli");
    }

    #[test]
    fn delete_session_returns_true_then_false() {
        let (_db_file, store) = store();
        let created = store.create_session("cli", "u1").unwrap();

        let first_delete = store.delete_session(&created.id).unwrap();
        let second_delete = store.delete_session(&created.id).unwrap();

        assert!(first_delete);
        assert!(!second_delete);
    }

    #[test]
    fn append_message_creates_message_with_expected_fields() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();

        let message = store
            .append_message(
                &session.id,
                MessageRole::Assistant,
                "Hello",
                Some(10),
                Some(20),
            )
            .unwrap();

        assert_eq!(message.session_id, session.id);
        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.content, "Hello");
        assert_eq!(message.input_tokens, Some(10));
        assert_eq!(message.output_tokens, Some(20));
    }

    #[test]
    fn get_messages_returns_chronological_order() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();
        store
            .append_message(&session.id, MessageRole::User, "first", None, None)
            .unwrap();
        store
            .append_message(&session.id, MessageRole::Assistant, "second", None, None)
            .unwrap();

        let messages = store.get_messages(&session.id, None).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].content, "second");
    }

    #[test]
    fn get_messages_with_limit_respects_limit() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m1", None, None)
            .unwrap();
        store
            .append_message(&session.id, MessageRole::Assistant, "m2", None, None)
            .unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m3", None, None)
            .unwrap();

        let messages = store.get_messages(&session.id, Some(2)).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "m2");
        assert_eq!(messages[1].content, "m3");
    }

    #[test]
    fn count_messages_returns_correct_count() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m1", None, None)
            .unwrap();
        store
            .append_message(&session.id, MessageRole::Assistant, "m2", None, None)
            .unwrap();

        let count = store.count_messages(&session.id).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn delete_messages_before_deletes_expected_messages() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();
        let first = store
            .append_message(&session.id, MessageRole::User, "m1", None, None)
            .unwrap();
        let second = store
            .append_message(&session.id, MessageRole::Assistant, "m2", None, None)
            .unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m3", None, None)
            .unwrap();

        let deleted = store
            .delete_messages_before(&session.id, &second.id)
            .unwrap();
        let remaining = store.get_messages(&session.id, None).unwrap();

        assert_eq!(deleted, 2);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].content, "m3");

        let still_gone = store.get_messages(&session.id, Some(10)).unwrap();
        assert!(still_gone.iter().all(|msg| msg.id != first.id));
    }
}
