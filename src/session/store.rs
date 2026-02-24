use super::types::{ChatMessage, MessageRole, Session, SessionState};
use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqliteRow};
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

/// Async session persistence contract.
pub trait SessionStore: Send + Sync {
    fn create_session<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Session>> + Send + 'a>>;

    fn get_session<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Session>>> + Send + 'a>>;

    fn get_or_create_session<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Session>> + Send + 'a>>;

    fn list_sessions<'a>(
        &'a self,
        channel: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Session>>> + Send + 'a>>;

    fn delete_session<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>>;

    fn update_session_state<'a>(
        &'a self,
        id: &'a str,
        state: SessionState,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    fn update_metadata<'a>(
        &'a self,
        id: &'a str,
        metadata: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    fn append_message<'a>(
        &'a self,
        session_id: &'a str,
        role: MessageRole,
        content: &'a str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Pin<Box<dyn Future<Output = Result<ChatMessage>> + Send + 'a>>;

    fn get_messages<'a>(
        &'a self,
        session_id: &'a str,
        limit: Option<usize>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ChatMessage>>> + Send + 'a>>;

    fn count_messages<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>>;

    fn delete_messages_before<'a>(
        &'a self,
        session_id: &'a str,
        before_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>>;
}

/// SQLite-backed session store using sqlx async pool.
pub struct SqliteSessionStore {
    pool: SqlitePool,
}

const SESSION_SCHEMA_META_TABLE: &str = "
CREATE TABLE IF NOT EXISTS session_schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";
const SESSION_SCHEMA_VERSION_KEY: &str = "session_schema_version";
const SESSION_SCHEMA_VERSION: u32 = 1;

async fn ensure_session_schema_version(pool: &SqlitePool) -> Result<()> {
    sqlx::query(SESSION_SCHEMA_META_TABLE)
        .execute(pool)
        .await
        .context("create session_schema_meta table")?;

    let stored_version: Option<(String,)> =
        sqlx::query_as("SELECT value FROM session_schema_meta WHERE key = $1")
            .bind(SESSION_SCHEMA_VERSION_KEY)
            .fetch_optional(pool)
            .await
            .context("load session schema version")?;

    if let Some((value,)) = stored_version {
        let parsed = value
            .parse::<u32>()
            .with_context(|| format!("invalid session schema version value: {value}"))?;
        anyhow::ensure!(
            parsed == SESSION_SCHEMA_VERSION,
            "incompatible session schema version: stored={parsed}, expected={SESSION_SCHEMA_VERSION}. \
compatibility is disabled; remove session DB and restart."
        );
        return Ok(());
    }

    let legacy_table_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)
         FROM sqlite_master
         WHERE type = 'table'
           AND name IN ('sessions', 'chat_messages')",
    )
    .fetch_one(pool)
    .await
    .context("detect legacy session tables")?;

    if legacy_table_count.0 > 0 {
        anyhow::bail!(
            "legacy session database detected without schema version metadata. \
compatibility is disabled; remove session DB and restart."
        );
    }

    sqlx::query("INSERT INTO session_schema_meta (key, value) VALUES ($1, $2)")
        .bind(SESSION_SCHEMA_VERSION_KEY)
        .bind(SESSION_SCHEMA_VERSION.to_string())
        .execute(pool)
        .await
        .context("persist session schema version")?;

    Ok(())
}

impl SqliteSessionStore {
    /// Create a new store with an existing pool and run migrations.
    pub async fn new(pool: SqlitePool) -> Result<Self> {
        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await?;

        ensure_session_schema_version(&pool).await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS sessions (
                 id TEXT PRIMARY KEY,
                 channel TEXT NOT NULL,
                 user_id TEXT NOT NULL,
                 state TEXT NOT NULL DEFAULT 'active',
                 model TEXT,
                 metadata TEXT,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL,
                 UNIQUE(channel, user_id, state)
             )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_messages (
                 id TEXT PRIMARY KEY,
                 session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 role TEXT NOT NULL,
                 content TEXT NOT NULL,
                 input_tokens INTEGER,
                 output_tokens INTEGER,
                 created_at TEXT NOT NULL
             )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chat_messages_session
                 ON chat_messages(session_id, created_at)",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    /// Access the underlying pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn state_to_str(state: SessionState) -> &'static str {
    match state {
        SessionState::Active => "active",
        SessionState::Archived => "archived",
        SessionState::Compacted => "compacted",
    }
}

fn str_to_state(value: &str) -> Result<SessionState> {
    match value {
        "active" => Ok(SessionState::Active),
        "archived" => Ok(SessionState::Archived),
        "compacted" => Ok(SessionState::Compacted),
        _ => anyhow::bail!("unknown session state: {value}"),
    }
}

fn role_to_str(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
    }
}

fn str_to_role(value: &str) -> Result<MessageRole> {
    match value {
        "user" => Ok(MessageRole::User),
        "assistant" => Ok(MessageRole::Assistant),
        "system" => Ok(MessageRole::System),
        _ => anyhow::bail!("unknown message role: {value}"),
    }
}

fn map_session_row(row: &SqliteRow) -> Result<Session> {
    let state_raw: String = row.try_get("state")?;
    let metadata_raw: Option<String> = row.try_get("metadata")?;
    let metadata = metadata_raw
        .map(|value| serde_json::from_str::<serde_json::Value>(&value))
        .transpose()
        .context("deserialize session metadata")?;

    Ok(Session {
        id: row.try_get("id")?,
        channel: row.try_get("channel")?,
        user_id: row.try_get("user_id")?,
        state: str_to_state(&state_raw)?,
        model: row.try_get("model")?,
        metadata,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_chat_message_row(row: &SqliteRow) -> Result<ChatMessage> {
    let role_raw: String = row.try_get("role")?;
    let input_tokens: Option<i64> = row.try_get("input_tokens")?;
    let output_tokens: Option<i64> = row.try_get("output_tokens")?;

    Ok(ChatMessage {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        role: str_to_role(&role_raw)?,
        content: row.try_get("content")?,
        #[allow(clippy::cast_sign_loss)]
        input_tokens: input_tokens.map(|v| v as u64),
        #[allow(clippy::cast_sign_loss)]
        output_tokens: output_tokens.map(|v| v as u64),
        created_at: row.try_get("created_at")?,
    })
}

impl SessionStore for SqliteSessionStore {
    fn create_session<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Session>> + Send + 'a>> {
        Box::pin(async move {
            let session_id = Uuid::new_v4().to_string();
            let timestamp = Utc::now().to_rfc3339();
            let state_str = state_to_str(SessionState::Active);

            sqlx::query(
                "INSERT INTO sessions (id, channel, user_id, state, model, metadata, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, NULL, NULL, $5, $5)",
            )
            .bind(&session_id)
            .bind(channel)
            .bind(user_id)
            .bind(state_str)
            .bind(&timestamp)
            .execute(&self.pool)
            .await?;

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
        })
    }

    fn get_session<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Session>>> + Send + 'a>> {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
                 FROM sessions
                 WHERE id = $1",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("query session by id")?;

            row.map(|r| map_session_row(&r)).transpose()
        })
    }

    fn get_or_create_session<'a>(
        &'a self,
        channel: &'a str,
        user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Session>> + Send + 'a>> {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
                 FROM sessions
                 WHERE channel = $1 AND user_id = $2 AND state = 'active'
                 ORDER BY updated_at DESC
                 LIMIT 1",
            )
            .bind(channel)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;

            if let Some(r) = row {
                return map_session_row(&r);
            }

            self.create_session(channel, user_id).await
        })
    }

    fn list_sessions<'a>(
        &'a self,
        channel: Option<&'a str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Session>>> + Send + 'a>> {
        Box::pin(async move {
            let rows = if let Some(channel_name) = channel {
                sqlx::query(
                    "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
                     FROM sessions
                     WHERE channel = $1
                     ORDER BY updated_at DESC",
                )
                .bind(channel_name)
                .fetch_all(&self.pool)
                .await?
            } else {
                sqlx::query(
                    "SELECT id, channel, user_id, state, model, metadata, created_at, updated_at
                     FROM sessions
                     ORDER BY updated_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            };

            rows.iter().map(map_session_row).collect()
        })
    }

    fn delete_session<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await?;
            Ok(result.rows_affected() > 0)
        })
    }

    fn update_session_state<'a>(
        &'a self,
        id: &'a str,
        state: SessionState,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let timestamp = Utc::now().to_rfc3339();
            sqlx::query(
                "UPDATE sessions
                 SET state = $1, updated_at = $2
                 WHERE id = $3",
            )
            .bind(state_to_str(state))
            .bind(&timestamp)
            .bind(id)
            .execute(&self.pool)
            .await?;
            Ok(())
        })
    }

    fn update_metadata<'a>(
        &'a self,
        id: &'a str,
        metadata: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let timestamp = Utc::now().to_rfc3339();
            let metadata_str = serde_json::to_string(&metadata)?;
            sqlx::query(
                "UPDATE sessions
                 SET metadata = $1, updated_at = $2
                 WHERE id = $3",
            )
            .bind(&metadata_str)
            .bind(&timestamp)
            .bind(id)
            .execute(&self.pool)
            .await?;
            Ok(())
        })
    }

    fn append_message<'a>(
        &'a self,
        session_id: &'a str,
        role: MessageRole,
        content: &'a str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Pin<Box<dyn Future<Output = Result<ChatMessage>> + Send + 'a>> {
        Box::pin(async move {
            let message_id = Uuid::new_v4().to_string();
            let created_at = Utc::now().to_rfc3339();
            #[allow(clippy::cast_possible_wrap)]
            let input_tokens_i64 = input_tokens.map(|v| v as i64);
            #[allow(clippy::cast_possible_wrap)]
            let output_tokens_i64 = output_tokens.map(|v| v as i64);

            sqlx::query(
                "INSERT INTO chat_messages (id, session_id, role, content, input_tokens, output_tokens, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(&message_id)
            .bind(session_id)
            .bind(role_to_str(role))
            .bind(content)
            .bind(input_tokens_i64)
            .bind(output_tokens_i64)
            .bind(&created_at)
            .execute(&self.pool)
            .await?;

            sqlx::query(
                "UPDATE sessions
                 SET updated_at = $1
                 WHERE id = $2",
            )
            .bind(&created_at)
            .bind(session_id)
            .execute(&self.pool)
            .await?;

            Ok(ChatMessage {
                id: message_id,
                session_id: session_id.to_string(),
                role,
                content: content.to_string(),
                input_tokens,
                output_tokens,
                created_at,
            })
        })
    }

    fn get_messages<'a>(
        &'a self,
        session_id: &'a str,
        limit: Option<usize>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<ChatMessage>>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(limit_count) = limit {
                #[allow(clippy::cast_possible_wrap)]
                let limit_i64 = limit_count as i64;

                let rows = sqlx::query(
                    "SELECT id, session_id, role, content, input_tokens, output_tokens, created_at
                     FROM chat_messages
                     WHERE session_id = $1
                     ORDER BY created_at DESC
                     LIMIT $2",
                )
                .bind(session_id)
                .bind(limit_i64)
                .fetch_all(&self.pool)
                .await?;

                let mut messages: Vec<ChatMessage> = rows
                    .iter()
                    .map(map_chat_message_row)
                    .collect::<Result<_>>()?;
                messages.reverse();
                Ok(messages)
            } else {
                let rows = sqlx::query(
                    "SELECT id, session_id, role, content, input_tokens, output_tokens, created_at
                     FROM chat_messages
                     WHERE session_id = $1
                     ORDER BY created_at ASC",
                )
                .bind(session_id)
                .fetch_all(&self.pool)
                .await?;

                rows.iter().map(map_chat_message_row).collect()
            }
        })
    }

    fn count_messages<'a>(
        &'a self,
        session_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>> {
        Box::pin(async move {
            let row =
                sqlx::query("SELECT COUNT(*) as cnt FROM chat_messages WHERE session_id = $1")
                    .bind(session_id)
                    .fetch_one(&self.pool)
                    .await?;

            let count: i64 = row.try_get("cnt")?;
            usize::try_from(count).context("convert message count to usize")
        })
    }

    fn delete_messages_before<'a>(
        &'a self,
        session_id: &'a str,
        before_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>> {
        Box::pin(async move {
            let result = sqlx::query(
                "DELETE FROM chat_messages
                 WHERE session_id = $1
                   AND created_at <= (
                       SELECT created_at
                       FROM chat_messages
                       WHERE id = $2 AND session_id = $1
                   )",
            )
            .bind(session_id)
            .bind(before_id)
            .execute(&self.pool)
            .await?;

            #[allow(clippy::cast_possible_truncation)]
            Ok(result.rows_affected() as usize)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SESSION_SCHEMA_META_TABLE, SESSION_SCHEMA_VERSION_KEY, SessionStore, SqliteSessionStore,
    };
    use crate::session::types::{MessageRole, SessionState};
    use sqlx::SqlitePool;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn store() -> SqliteSessionStore {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        SqliteSessionStore::new(pool).await.unwrap()
    }

    #[tokio::test]
    async fn create_session_returns_valid_session() {
        let store = store().await;
        let session = store.create_session("cli", "user-1").await.unwrap();

        assert!(!session.id.is_empty());
        assert_eq!(session.channel, "cli");
        assert_eq!(session.user_id, "user-1");
        assert_eq!(session.state, SessionState::Active);
    }

    #[tokio::test]
    async fn get_session_finds_existing_and_none_for_missing() {
        let store = store().await;
        let created = store.create_session("cli", "user-1").await.unwrap();

        let found = store.get_session(&created.id).await.unwrap();
        let missing = store.get_session("missing-id").await.unwrap();

        assert!(found.is_some());
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn get_or_create_session_returns_existing_active_session() {
        let store = store().await;
        let first = store
            .get_or_create_session("discord", "user-99")
            .await
            .unwrap();
        let second = store
            .get_or_create_session("discord", "user-99")
            .await
            .unwrap();

        assert_eq!(first.id, second.id);
    }

    #[tokio::test]
    async fn list_sessions_returns_all_and_filters_by_channel() {
        let store = store().await;
        store.create_session("cli", "u1").await.unwrap();
        store.create_session("discord", "u2").await.unwrap();

        let all = store.list_sessions(None).await.unwrap();
        let cli_only = store.list_sessions(Some("cli")).await.unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(cli_only.len(), 1);
        assert_eq!(cli_only[0].channel, "cli");
    }

    #[tokio::test]
    async fn delete_session_returns_true_then_false() {
        let store = store().await;
        let created = store.create_session("cli", "u1").await.unwrap();

        let first_delete = store.delete_session(&created.id).await.unwrap();
        let second_delete = store.delete_session(&created.id).await.unwrap();

        assert!(first_delete);
        assert!(!second_delete);
    }

    #[tokio::test]
    async fn update_metadata_persists() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        let meta = serde_json::json!({"model": "gpt-4", "tokens": 1000});
        store
            .update_metadata(&session.id, meta.clone())
            .await
            .unwrap();

        let loaded = store.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(loaded.metadata, Some(meta));
    }

    #[tokio::test]
    async fn append_message_creates_message_with_expected_fields() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();

        let message = store
            .append_message(
                &session.id,
                MessageRole::Assistant,
                "Hello",
                Some(10),
                Some(20),
            )
            .await
            .unwrap();

        assert_eq!(message.session_id, session.id);
        assert_eq!(message.role, MessageRole::Assistant);
        assert_eq!(message.content, "Hello");
        assert_eq!(message.input_tokens, Some(10));
        assert_eq!(message.output_tokens, Some(20));
    }

    #[tokio::test]
    async fn get_messages_returns_chronological_order() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        store
            .append_message(&session.id, MessageRole::User, "first", None, None)
            .await
            .unwrap();
        store
            .append_message(&session.id, MessageRole::Assistant, "second", None, None)
            .await
            .unwrap();

        let messages = store.get_messages(&session.id, None).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].content, "second");
    }

    #[tokio::test]
    async fn get_messages_with_limit_respects_limit() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m1", None, None)
            .await
            .unwrap();
        store
            .append_message(&session.id, MessageRole::Assistant, "m2", None, None)
            .await
            .unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m3", None, None)
            .await
            .unwrap();

        let messages = store.get_messages(&session.id, Some(2)).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "m2");
        assert_eq!(messages[1].content, "m3");
    }

    #[tokio::test]
    async fn count_messages_returns_correct_count() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m1", None, None)
            .await
            .unwrap();
        store
            .append_message(&session.id, MessageRole::Assistant, "m2", None, None)
            .await
            .unwrap();

        let count = store.count_messages(&session.id).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn delete_messages_before_deletes_expected_messages() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        let first = store
            .append_message(&session.id, MessageRole::User, "m1", None, None)
            .await
            .unwrap();
        let second = store
            .append_message(&session.id, MessageRole::Assistant, "m2", None, None)
            .await
            .unwrap();
        store
            .append_message(&session.id, MessageRole::User, "m3", None, None)
            .await
            .unwrap();

        let deleted = store
            .delete_messages_before(&session.id, &second.id)
            .await
            .unwrap();
        let remaining = store.get_messages(&session.id, None).await.unwrap();

        assert_eq!(deleted, 2);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].content, "m3");

        let still_gone = store.get_messages(&session.id, Some(10)).await.unwrap();
        assert!(still_gone.iter().all(|msg| msg.id != first.id));
    }

    #[tokio::test]
    async fn new_rejects_legacy_unversioned_session_database() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE sessions (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();

        let err = match SqliteSessionStore::new(pool).await {
            Ok(_) => panic!("legacy unversioned session DB must fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("legacy session database detected without schema version metadata"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn new_rejects_session_schema_version_mismatch() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(SESSION_SCHEMA_META_TABLE)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO session_schema_meta (key, value) VALUES ($1, $2)")
            .bind(SESSION_SCHEMA_VERSION_KEY)
            .bind("999")
            .execute(&pool)
            .await
            .unwrap();

        let err = match SqliteSessionStore::new(pool).await {
            Ok(_) => panic!("session schema version mismatch must fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("incompatible session schema version"),
            "unexpected error: {err}"
        );
    }
}
