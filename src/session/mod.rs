pub mod compaction;
pub mod manager;
pub mod store;
pub mod types;

pub use compaction::{CompactionConfig, CompactionLevel, CompactionResult, compact_session};
pub use manager::SessionManager;
pub use store::{SessionStore, SqliteSessionStore};
pub use types::{ChatMessage, MessageRole, Session, SessionConfig, SessionState};
