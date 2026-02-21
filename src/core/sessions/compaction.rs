use super::store::{SessionStore, SqliteSessionStore};
use super::types::{MessageRole, SessionState};
use crate::utils::text::truncate_with_ellipsis;
use anyhow::Result;

pub fn compact_session(
    store: &SqliteSessionStore,
    session_id: &str,
    threshold: usize,
) -> Result<bool> {
    let message_count = store.count_messages(session_id)?;
    if message_count <= threshold {
        return Ok(false);
    }

    let messages = store.get_messages(session_id, None)?;
    let keep_count = threshold / 2;
    let split_index = messages.len().saturating_sub(keep_count);
    let to_summarize = &messages[..split_index];
    if to_summarize.is_empty() {
        return Ok(false);
    }

    let mut summary_parts = Vec::with_capacity(to_summarize.len());
    for message in to_summarize {
        let role_label = match message.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        };
        summary_parts.push(format!(
            "{role_label}: {}",
            truncate_with_ellipsis(&message.content, 200)
        ));
    }

    let summary = format!(
        "[Session history summary ({} messages compacted)]\n{}",
        to_summarize.len(),
        summary_parts.join("\n")
    );

    if let Some(cutoff_message) = to_summarize.last() {
        store.delete_messages_before(session_id, &cutoff_message.id)?;
    }
    store.append_message(session_id, MessageRole::System, &summary, None, None)?;
    store.update_session_state(session_id, SessionState::Compacted)?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::compact_session;
    use crate::core::sessions::store::{SessionStore, SqliteSessionStore};
    use crate::core::sessions::types::{MessageRole, SessionState};
    use tempfile::NamedTempFile;

    fn store() -> (NamedTempFile, SqliteSessionStore) {
        let db_file = NamedTempFile::new().unwrap();
        let store = SqliteSessionStore::new(db_file.path()).unwrap();
        (db_file, store)
    }

    #[test]
    fn compact_below_threshold_returns_false() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();
        store
            .append_message(&session.id, MessageRole::User, "hello", None, None)
            .unwrap();

        let compacted = compact_session(&store, &session.id, 10).unwrap();
        assert!(!compacted);
    }

    #[test]
    fn compact_above_threshold_summarizes_and_returns_true() {
        let (_db_file, store) = store();
        let session = store.create_session("cli", "u1").unwrap();
        for index in 0..6 {
            let role = if index % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            };
            store
                .append_message(&session.id, role, &format!("msg-{index}"), None, None)
                .unwrap();
        }

        let compacted = compact_session(&store, &session.id, 4).unwrap();
        assert!(compacted);

        let messages = store.get_messages(&session.id, None).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].role, MessageRole::System);
        assert!(messages[2].content.contains("Session history summary"));

        let session_after = store.get_session(&session.id).unwrap().unwrap();
        assert_eq!(session_after.state, SessionState::Compacted);
    }
}
