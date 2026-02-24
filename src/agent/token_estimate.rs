use crate::llm::types::{ContentBlock, ProviderMessage};

/// Estimate token count from text using the chars/4 heuristic.
///
/// Uses ceiling division to avoid underestimating by a fraction.
pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    chars.div_ceil(4)
}

/// Estimate total tokens for a message history.
///
/// Sums text content blocks plus a small overhead per message (4 tokens)
/// to account for message framing (role label, separators, etc.).
pub fn estimate_message_tokens(messages: &[ProviderMessage]) -> u64 {
    const PER_MESSAGE_OVERHEAD: u64 = 4;

    messages
        .iter()
        .map(|msg| {
            let content_tokens: u64 = msg
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => estimate_tokens(text),
                    ContentBlock::ToolUse { name, input, .. } => {
                        let input_str = serde_json::to_string(input).unwrap_or_default();
                        estimate_tokens(name) + estimate_tokens(&input_str)
                    }
                    ContentBlock::ToolResult { content, .. } => estimate_tokens(content),
                    ContentBlock::Image { .. } => 256, // fixed estimate for images
                })
                .sum();
            content_tokens + PER_MESSAGE_OVERHEAD
        })
        .sum()
}

/// Provider-specific adjustment factor for token estimates.
///
/// Different tokenizers have slightly different token-to-character ratios.
/// Multiply the heuristic estimate by this factor for a more accurate count.
pub fn provider_token_factor(provider_name: &str) -> f64 {
    match provider_name {
        "openai" => 1.05,
        "gemini" => 0.95,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{ContentBlock, MessageRole, ProviderMessage};

    // ── estimate_tokens ─────────────────────────────────────────────────────

    #[test]
    fn empty_string_returns_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn single_char_returns_one() {
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn four_chars_returns_one() {
        assert_eq!(estimate_tokens("abcd"), 1);
    }

    #[test]
    fn five_chars_returns_two() {
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn eight_chars_returns_two() {
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn multibyte_unicode_counts_chars_not_bytes() {
        // 4 unicode characters, each multi-byte
        let text = "\u{1F600}\u{1F601}\u{1F602}\u{1F603}";
        assert_eq!(text.chars().count(), 4);
        assert_eq!(estimate_tokens(text), 1);
    }

    #[test]
    fn longer_text_estimate() {
        // "Hello, world!" is 13 chars => ceil(13/4) = 4
        assert_eq!(estimate_tokens("Hello, world!"), 4);
    }

    // ── estimate_message_tokens ─────────────────────────────────────────────

    #[test]
    fn empty_messages_returns_zero() {
        assert_eq!(estimate_message_tokens(&[]), 0);
    }

    #[test]
    fn single_text_message() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: "Hello".into(),
            }],
        }];
        // "Hello" = 5 chars => ceil(5/4) = 2, plus 4 overhead = 6
        assert_eq!(estimate_message_tokens(&messages), 6);
    }

    #[test]
    fn multiple_messages() {
        let messages = vec![
            ProviderMessage {
                role: MessageRole::User,
                content: vec![ContentBlock::Text { text: "Hi".into() }],
            },
            ProviderMessage {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hello there".into(),
                }],
            },
        ];
        // "Hi" = 2 chars => ceil(2/4) = 1, + 4 = 5
        // "Hello there" = 11 chars => ceil(11/4) = 3, + 4 = 7
        // Total: 12
        assert_eq!(estimate_message_tokens(&messages), 12);
    }

    #[test]
    fn message_with_tool_use_block() {
        let messages = vec![ProviderMessage {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "call-1".into(),
                name: "shell".into(),
                input: serde_json::json!({"command": "ls"}),
            }],
        }];
        // name "shell" = 5 chars => ceil(5/4) = 2
        // input JSON '{"command":"ls"}' = 16 chars => ceil(16/4) = 4
        // Total content: 6, + 4 overhead = 10
        let result = estimate_message_tokens(&messages);
        assert!(
            result > 4,
            "should include content + overhead, got {result}"
        );
    }

    #[test]
    fn message_with_tool_result_block() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "call-1".into(),
                content: "file.txt\nREADME.md".into(),
                is_error: false,
            }],
        }];
        // "file.txt\nREADME.md" = 18 chars => ceil(18/4) = 5, + 4 = 9
        assert_eq!(estimate_message_tokens(&messages), 9);
    }

    #[test]
    fn message_with_image_block() {
        let messages = vec![ProviderMessage {
            role: MessageRole::User,
            content: vec![ContentBlock::Image {
                source: crate::llm::types::ImageSource::url("https://example.com/img.png"),
            }],
        }];
        // Image = 256 fixed, + 4 overhead = 260
        assert_eq!(estimate_message_tokens(&messages), 260);
    }

    // ── provider_token_factor ───────────────────────────────────────────────

    #[test]
    fn anthropic_factor_is_one() {
        assert!((provider_token_factor("anthropic") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn openai_factor_is_above_one() {
        assert!(provider_token_factor("openai") > 1.0);
        assert!((provider_token_factor("openai") - 1.05).abs() < f64::EPSILON);
    }

    #[test]
    fn gemini_factor_is_below_one() {
        assert!(provider_token_factor("gemini") < 1.0);
        assert!((provider_token_factor("gemini") - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn unknown_provider_defaults_to_one() {
        assert!((provider_token_factor("custom-llm") - 1.0).abs() < f64::EPSILON);
        assert!((provider_token_factor("") - 1.0).abs() < f64::EPSILON);
    }
}
