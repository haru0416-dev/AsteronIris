use super::store::SessionStore;
use super::types::{MessageRole, SessionState};
use crate::llm::traits::Provider;
use crate::utils::text::truncate_with_ellipsis;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Tiered compaction thresholds expressed as fractions of the context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Absolute message threshold that triggers compaction.
    pub threshold: usize,
    /// Fraction of context window for light compaction (default 0.50).
    pub tier_light: f64,
    /// Fraction of context window for moderate compaction (default 0.75).
    pub tier_moderate: f64,
    /// Fraction of context window for aggressive compaction (default 0.95).
    pub tier_aggressive: f64,
    /// Maximum context window in tokens (used for tier calculation).
    pub context_window_tokens: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            threshold: 50,
            tier_light: 0.50,
            tier_moderate: 0.75,
            tier_aggressive: 0.95,
            context_window_tokens: 128_000,
        }
    }
}

/// Describes the aggressiveness level chosen for compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionLevel {
    /// Below the light tier: keep half the messages.
    Light,
    /// Between light and moderate: keep a third.
    Moderate,
    /// Above the moderate tier: keep only a quarter.
    Aggressive,
}

/// Outcome returned after a compaction attempt.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub compacted: bool,
    pub messages_removed: usize,
    pub level: Option<CompactionLevel>,
    pub summary_injected: bool,
}

impl CompactionResult {
    fn skipped() -> Self {
        Self {
            compacted: false,
            messages_removed: 0,
            level: None,
            summary_injected: false,
        }
    }
}

/// Estimate total token usage of a session's messages (rough heuristic).
#[allow(clippy::cast_possible_truncation)]
fn estimate_tokens(messages: &[super::types::ChatMessage]) -> usize {
    messages
        .iter()
        .map(|m| {
            // Rough approximation: 4 characters per token on average
            m.content.len() / 4
                + m.input_tokens.unwrap_or(0) as usize
                + m.output_tokens.unwrap_or(0) as usize
        })
        .sum()
}

/// Determine compaction level based on token usage relative to tiers.
fn determine_level(token_estimate: usize, config: &CompactionConfig) -> CompactionLevel {
    let window = config.context_window_tokens;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let moderate_threshold = (window as f64 * config.tier_moderate) as usize;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let aggressive_threshold = (window as f64 * config.tier_aggressive) as usize;

    if token_estimate >= aggressive_threshold {
        CompactionLevel::Aggressive
    } else if token_estimate >= moderate_threshold {
        CompactionLevel::Moderate
    } else {
        CompactionLevel::Light
    }
}

/// Fraction of recent messages to keep based on compaction level.
fn keep_fraction(level: CompactionLevel) -> usize {
    match level {
        CompactionLevel::Light => 2,      // keep 1/2
        CompactionLevel::Moderate => 3,   // keep 1/3
        CompactionLevel::Aggressive => 4, // keep 1/4
    }
}

/// Compact a session by summarizing old messages and deleting them.
///
/// Uses tiered thresholds to determine how aggressively to compact.
/// The `provider` parameter is reserved for future LLM-based summarization;
/// currently a simple text summary is produced.
pub async fn compact_session(
    store: &dyn SessionStore,
    session_id: &str,
    _provider: Option<&dyn Provider>,
    config: &CompactionConfig,
) -> Result<CompactionResult> {
    let message_count = store.count_messages(session_id).await?;
    if message_count <= config.threshold {
        return Ok(CompactionResult::skipped());
    }

    let messages = store.get_messages(session_id, None).await?;
    let token_estimate = estimate_tokens(&messages);
    let level = determine_level(token_estimate, config);
    let divisor = keep_fraction(level);
    let keep_count = messages.len() / divisor;
    let split_index = messages.len().saturating_sub(keep_count);
    let to_summarize = &messages[..split_index];

    if to_summarize.is_empty() {
        return Ok(CompactionResult::skipped());
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
        "[Session history summary ({} messages compacted, level={level:?})]\n{}",
        to_summarize.len(),
        summary_parts.join("\n")
    );

    let messages_removed = to_summarize.len();

    if let Some(cutoff_message) = to_summarize.last() {
        store
            .delete_messages_before(session_id, &cutoff_message.id)
            .await?;
    }
    store
        .append_message(session_id, MessageRole::System, &summary, None, None)
        .await?;
    store
        .update_session_state(session_id, SessionState::Compacted)
        .await?;

    Ok(CompactionResult {
        compacted: true,
        messages_removed,
        level: Some(level),
        summary_injected: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::store::{SessionStore, SqliteSessionStore};
    use crate::session::types::{MessageRole, SessionState};
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
    async fn compact_below_threshold_returns_skipped() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        store
            .append_message(&session.id, MessageRole::User, "hello", None, None)
            .await
            .unwrap();

        let config = CompactionConfig {
            threshold: 10,
            ..CompactionConfig::default()
        };
        let result = compact_session(&store, &session.id, None, &config)
            .await
            .unwrap();
        assert!(!result.compacted);
    }

    #[tokio::test]
    async fn compact_above_threshold_summarizes_and_returns_compacted() {
        let store = store().await;
        let session = store.create_session("cli", "u1").await.unwrap();
        for index in 0..6 {
            let role = if index % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            };
            store
                .append_message(&session.id, role, &format!("msg-{index}"), None, None)
                .await
                .unwrap();
        }

        let config = CompactionConfig {
            threshold: 4,
            ..CompactionConfig::default()
        };
        let result = compact_session(&store, &session.id, None, &config)
            .await
            .unwrap();
        assert!(result.compacted);
        assert!(result.messages_removed > 0);
        assert!(result.level.is_some());

        let messages = store.get_messages(&session.id, None).await.unwrap();
        // Should have kept messages plus the injected summary
        assert!(messages.iter().any(|m| m.role == MessageRole::System));
        assert!(
            messages
                .iter()
                .any(|m| m.content.contains("Session history summary"))
        );

        let session_after = store.get_session(&session.id).await.unwrap().unwrap();
        assert_eq!(session_after.state, SessionState::Compacted);
    }

    #[test]
    fn determine_level_returns_correct_tiers() {
        let config = CompactionConfig {
            context_window_tokens: 100_000,
            tier_light: 0.50,
            tier_moderate: 0.75,
            tier_aggressive: 0.95,
            ..CompactionConfig::default()
        };

        assert_eq!(determine_level(40_000, &config), CompactionLevel::Light);
        assert_eq!(determine_level(80_000, &config), CompactionLevel::Moderate);
        assert_eq!(
            determine_level(96_000, &config),
            CompactionLevel::Aggressive
        );
    }
}
