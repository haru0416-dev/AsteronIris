use crate::agent::token_estimate;
use crate::llm::types::{ContentBlock, MessageRole, ProviderMessage};

/// Thresholds that control when and how aggressively context is compacted.
#[derive(Debug, Clone)]
pub struct CompactionThresholds {
    /// Ratio of used tokens to max that triggers light compaction.
    pub light_ratio: f64,
    /// Ratio of used tokens to max that triggers moderate compaction.
    pub moderate_ratio: f64,
    /// Ratio of used tokens to max that triggers aggressive compaction.
    pub aggressive_ratio: f64,
    /// Maximum context window size in tokens.
    pub max_context_tokens: u64,
}

impl Default for CompactionThresholds {
    fn default() -> Self {
        Self {
            light_ratio: 0.50,
            moderate_ratio: 0.75,
            aggressive_ratio: 0.95,
            max_context_tokens: 128_000,
        }
    }
}

/// Describes the aggressiveness level chosen for compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionLevel {
    None,
    Light,
    Moderate,
    Aggressive,
}

/// Assess which compaction level is appropriate for the given messages.
pub fn assess_compaction(
    messages: &[ProviderMessage],
    thresholds: &CompactionThresholds,
) -> CompactionLevel {
    let tokens = token_estimate::estimate_message_tokens(messages);
    #[allow(clippy::cast_precision_loss)]
    let ratio = tokens as f64 / thresholds.max_context_tokens as f64;

    if ratio >= thresholds.aggressive_ratio {
        CompactionLevel::Aggressive
    } else if ratio >= thresholds.moderate_ratio {
        CompactionLevel::Moderate
    } else if ratio >= thresholds.light_ratio {
        CompactionLevel::Light
    } else {
        CompactionLevel::None
    }
}

/// Compact messages by summarizing older ones textually.
///
/// Produces a simple text summary of the oldest messages and keeps the most
/// recent fraction. No LLM call is made; this is a heuristic compaction.
pub fn compact_messages(
    messages: &[ProviderMessage],
    level: CompactionLevel,
) -> Vec<ProviderMessage> {
    if level == CompactionLevel::None || messages.is_empty() {
        return messages.to_vec();
    }

    let keep_fraction = match level {
        CompactionLevel::Light => 2,      // keep 1/2
        CompactionLevel::Moderate => 3,   // keep 1/3
        CompactionLevel::Aggressive => 4, // keep 1/4
        CompactionLevel::None => unreachable!(),
    };

    let keep_count = messages.len() / keep_fraction;
    let split_at = messages.len().saturating_sub(keep_count.max(2));
    let to_summarize = &messages[..split_at];
    let to_keep = &messages[split_at..];

    if to_summarize.is_empty() {
        return messages.to_vec();
    }

    // Build simple text summary of compacted messages.
    let summary_parts: Vec<String> = to_summarize
        .iter()
        .map(|msg| {
            let role_label = match msg.role {
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::System => "System",
            };
            let text: String = msg
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            let truncated = if text.len() > 200 {
                format!("{}...", &text[..200])
            } else {
                text
            };
            format!("{role_label}: {truncated}")
        })
        .collect();

    let summary = format!(
        "[Context summary: {} messages compacted at level {level:?}]\n{}",
        to_summarize.len(),
        summary_parts.join("\n")
    );

    let mut result = vec![ProviderMessage {
        role: MessageRole::System,
        content: vec![ContentBlock::Text { text: summary }],
    }];
    result.extend_from_slice(to_keep);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(count: usize, char_count: usize) -> Vec<ProviderMessage> {
        (0..count)
            .map(|i| {
                let role = if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                };
                let text = "x".repeat(char_count);
                ProviderMessage {
                    role,
                    content: vec![ContentBlock::Text { text }],
                }
            })
            .collect()
    }

    #[test]
    fn assess_returns_none_below_threshold() {
        let thresholds = CompactionThresholds {
            max_context_tokens: 100_000,
            light_ratio: 0.50,
            ..CompactionThresholds::default()
        };
        // Each message: ~25 chars = ~7 tokens + 4 overhead = ~11 tokens.
        // 10 messages = ~110 tokens, well under 50_000.
        let messages = make_messages(10, 25);
        assert_eq!(
            assess_compaction(&messages, &thresholds),
            CompactionLevel::None
        );
    }

    #[test]
    fn assess_returns_light_at_light_ratio() {
        let thresholds = CompactionThresholds {
            max_context_tokens: 100,
            light_ratio: 0.50,
            moderate_ratio: 0.75,
            aggressive_ratio: 0.95,
        };
        // Need ~50 tokens. Each message with 180 chars ~= 45 tokens + 4 = 49.
        // 2 messages ~= 98 tokens, ratio = 0.98, that's aggressive.
        // Actually let's be more precise. Make it target the light zone.
        // 1 message with 180 chars: ceil(180/4) = 45 + 4 overhead = 49.
        // ratio = 49/100 = 0.49 < 0.50 => None. Need 51 tokens.
        // 1 message with 188 chars: ceil(188/4) = 47 + 4 = 51 => ratio = 0.51 => Light
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "a".repeat(188),
            }],
        }];
        assert_eq!(
            assess_compaction(&messages, &thresholds),
            CompactionLevel::Light
        );
    }

    #[test]
    fn assess_returns_moderate_at_moderate_ratio() {
        let thresholds = CompactionThresholds {
            max_context_tokens: 100,
            light_ratio: 0.50,
            moderate_ratio: 0.75,
            aggressive_ratio: 0.95,
        };
        // Need ~76 tokens for moderate. 1 message with 288 chars: ceil(288/4)=72 + 4 = 76.
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "b".repeat(288),
            }],
        }];
        assert_eq!(
            assess_compaction(&messages, &thresholds),
            CompactionLevel::Moderate
        );
    }

    #[test]
    fn assess_returns_aggressive_at_aggressive_ratio() {
        let thresholds = CompactionThresholds {
            max_context_tokens: 100,
            light_ratio: 0.50,
            moderate_ratio: 0.75,
            aggressive_ratio: 0.95,
        };
        // Need ~96 tokens for aggressive. 1 message with 368 chars: ceil(368/4)=92 + 4 = 96.
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "c".repeat(368),
            }],
        }];
        assert_eq!(
            assess_compaction(&messages, &thresholds),
            CompactionLevel::Aggressive
        );
    }

    #[test]
    fn compact_none_returns_original() {
        let messages = make_messages(4, 20);
        let result = compact_messages(&messages, CompactionLevel::None);
        assert_eq!(result.len(), messages.len());
    }

    #[test]
    fn compact_empty_returns_empty() {
        let result = compact_messages(&[], CompactionLevel::Aggressive);
        assert!(result.is_empty());
    }

    #[test]
    fn compact_light_preserves_recent_half() {
        let messages = make_messages(10, 20);
        let result = compact_messages(&messages, CompactionLevel::Light);

        // keep_count = 10/2 = 5, split_at = 10 - max(5, 2) = 5.
        // 5 summarized into 1 system message, 5 kept => 6 total.
        assert_eq!(result.len(), 6);
        assert_eq!(result[0].role, MessageRole::System);
        // The summary should mention the count.
        if let ContentBlock::Text { text } = &result[0].content[0] {
            assert!(text.contains("5 messages compacted"));
            assert!(text.contains("Light"));
        } else {
            panic!("expected text content block");
        }
    }

    #[test]
    fn compact_moderate_preserves_recent_third() {
        let messages = make_messages(12, 20);
        let result = compact_messages(&messages, CompactionLevel::Moderate);

        // keep_count = 12/3 = 4, split_at = 12 - max(4, 2) = 8.
        // 8 summarized, 4 kept => 5 total.
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].role, MessageRole::System);
    }

    #[test]
    fn compact_aggressive_preserves_recent_quarter() {
        let messages = make_messages(12, 20);
        let result = compact_messages(&messages, CompactionLevel::Aggressive);

        // keep_count = 12/4 = 3, split_at = 12 - max(3, 2) = 9.
        // 9 summarized, 3 kept => 4 total.
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].role, MessageRole::System);
    }

    #[test]
    fn compact_preserves_minimum_two_messages() {
        // With 3 messages and Aggressive (keep 1/4 = 0, clamped to 2).
        let messages = make_messages(3, 20);
        let result = compact_messages(&messages, CompactionLevel::Aggressive);

        // keep_count = 3/4 = 0, clamped to 2. split_at = 3 - 2 = 1.
        // 1 summarized, 2 kept => 3 total.
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, MessageRole::System);
    }

    #[test]
    fn compact_truncates_long_text_in_summary() {
        let long_text = "z".repeat(300);
        let messages = vec![
            ProviderMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: long_text.clone(),
                }],
            },
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "short".to_string(),
                }],
            },
            ProviderMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text {
                    text: "recent".to_string(),
                }],
            },
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "latest".to_string(),
                }],
            },
        ];

        let result = compact_messages(&messages, CompactionLevel::Light);

        // The summary should truncate the 300-char text to 200 + "..."
        if let ContentBlock::Text { text } = &result[0].content[0] {
            assert!(text.contains("..."));
            // Ensure it doesn't contain the full 300 chars.
            assert!(!text.contains(&long_text));
        } else {
            panic!("expected text content block");
        }
    }

    #[test]
    fn default_thresholds_are_reasonable() {
        let t = CompactionThresholds::default();
        assert!((t.light_ratio - 0.50).abs() < f64::EPSILON);
        assert!((t.moderate_ratio - 0.75).abs() < f64::EPSILON);
        assert!((t.aggressive_ratio - 0.95).abs() < f64::EPSILON);
        assert_eq!(t.max_context_tokens, 128_000);
    }
}
