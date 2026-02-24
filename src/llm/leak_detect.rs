use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

/// A detected secret leak in scanned text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedLeak {
    /// Human-readable label for the kind of secret found.
    pub kind: String,
    /// The matched fragment (may be truncated for display safety).
    pub matched: String,
    /// The encoding in which the secret was found.
    pub encoding: LeakEncoding,
}

/// Encoding in which a secret pattern was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakEncoding {
    Plain,
    UrlEncoded,
    Base64,
    Hex,
}

/// Known prefix patterns and their human-readable labels.
const SECRET_PREFIXES: &[(&str, &str)] = &[
    ("sk-", "OpenAI/Stripe API key"),
    ("ghp_", "GitHub personal access token"),
    ("github_pat_", "GitHub fine-grained PAT"),
    ("gho_", "GitHub OAuth token"),
    ("ghu_", "GitHub user-to-server token"),
    ("ghs_", "GitHub server-to-server token"),
    ("AKIA", "AWS access key"),
    ("ASIA", "AWS temporary access key"),
    ("xoxb-", "Slack bot token"),
    ("xoxp-", "Slack user token"),
    ("xoxs-", "Slack session token"),
    ("xoxa-", "Slack app token"),
    ("xapp-", "Slack app-level token"),
    ("hf_", "Hugging Face token"),
    ("glpat-", "GitLab personal access token"),
    ("AGE-SECRET-KEY-", "age encryption key"),
    ("GOCSPX-", "Google OAuth client secret"),
    ("AIza", "Google API key"),
    ("ya29.", "Google OAuth access token"),
    ("sshpass-", "SSH password token"),
    ("eyJ", "JWT token"),
];

/// Minimum length of the token portion (after prefix) to consider it a match.
const MIN_TOKEN_TAIL: usize = 8;

/// Scan `text` for leaked secrets across multiple encodings.
///
/// Checks for known secret prefixes in:
/// - Plain text
/// - URL-encoded form (`sk-` as `sk%2D`)
/// - Base64-encoded content (decoded, then scanned)
/// - Hex-encoded content (decoded, then scanned)
///
/// Returns a list of all detected leaks.
pub fn scan_for_leaks(text: &str) -> Vec<DetectedLeak> {
    let mut leaks = Vec::new();

    scan_plain(text, &mut leaks);
    scan_url_encoded(text, &mut leaks);
    scan_base64(text, &mut leaks);
    scan_hex(text, &mut leaks);

    leaks
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '+' | '/' | '=')
}

fn extract_token_at(text: &str, start: usize) -> &str {
    let rest = &text[start..];
    let end = rest.find(|c: char| !is_token_char(c)).unwrap_or(rest.len());
    &rest[..end]
}

fn scan_plain(text: &str, leaks: &mut Vec<DetectedLeak>) {
    for &(prefix, kind) in SECRET_PREFIXES {
        let mut search_from = 0;
        while let Some(pos) = text[search_from..].find(prefix) {
            let abs_pos = search_from + pos;
            let token = extract_token_at(text, abs_pos);
            let tail_len = token.len().saturating_sub(prefix.len());
            if tail_len >= MIN_TOKEN_TAIL {
                leaks.push(DetectedLeak {
                    kind: kind.to_string(),
                    matched: truncate_match(token),
                    encoding: LeakEncoding::Plain,
                });
            }
            search_from = abs_pos + prefix.len();
        }
    }
}

fn scan_url_encoded(text: &str, leaks: &mut Vec<DetectedLeak>) {
    // URL-decode the entire text, then scan the decoded version.
    let decoded = url_decode(text);
    if decoded == text {
        // No URL-encoded sequences present; skip to avoid duplicates.
        return;
    }

    for &(prefix, kind) in SECRET_PREFIXES {
        let mut search_from = 0;
        while let Some(pos) = decoded[search_from..].find(prefix) {
            let abs_pos = search_from + pos;
            let token = extract_token_at(&decoded, abs_pos);
            let tail_len = token.len().saturating_sub(prefix.len());
            if tail_len >= MIN_TOKEN_TAIL {
                // Only report if this token was NOT already found as plain.
                if !text.contains(token) {
                    leaks.push(DetectedLeak {
                        kind: kind.to_string(),
                        matched: truncate_match(token),
                        encoding: LeakEncoding::UrlEncoded,
                    });
                }
            }
            search_from = abs_pos + prefix.len();
        }
    }
}

fn scan_base64(text: &str, leaks: &mut Vec<DetectedLeak>) {
    // Find base64-like runs and attempt decode.
    for candidate in extract_base64_candidates(text) {
        let Ok(decoded_bytes) = BASE64_STANDARD.decode(candidate) else {
            continue;
        };
        let Ok(decoded) = std::str::from_utf8(&decoded_bytes) else {
            continue;
        };
        for &(prefix, kind) in SECRET_PREFIXES {
            if decoded.contains(prefix) {
                let mut search_from = 0;
                while let Some(pos) = decoded[search_from..].find(prefix) {
                    let abs_pos = search_from + pos;
                    let token = extract_token_at(decoded, abs_pos);
                    let tail_len = token.len().saturating_sub(prefix.len());
                    if tail_len >= MIN_TOKEN_TAIL {
                        leaks.push(DetectedLeak {
                            kind: kind.to_string(),
                            matched: truncate_match(token),
                            encoding: LeakEncoding::Base64,
                        });
                    }
                    search_from = abs_pos + prefix.len();
                }
            }
        }
    }
}

fn scan_hex(text: &str, leaks: &mut Vec<DetectedLeak>) {
    for candidate in extract_hex_candidates(text) {
        let Ok(decoded_bytes) = hex::decode(candidate) else {
            continue;
        };
        let Ok(decoded) = std::str::from_utf8(&decoded_bytes) else {
            continue;
        };
        for &(prefix, kind) in SECRET_PREFIXES {
            if decoded.contains(prefix) {
                let mut search_from = 0;
                while let Some(pos) = decoded[search_from..].find(prefix) {
                    let abs_pos = search_from + pos;
                    let token = extract_token_at(decoded, abs_pos);
                    let tail_len = token.len().saturating_sub(prefix.len());
                    if tail_len >= MIN_TOKEN_TAIL {
                        leaks.push(DetectedLeak {
                            kind: kind.to_string(),
                            matched: truncate_match(token),
                            encoding: LeakEncoding::Hex,
                        });
                    }
                    search_from = abs_pos + prefix.len();
                }
            }
        }
    }
}

/// Simple percent-decode (handles `%XX` sequences).
fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                } else {
                    result.push('%');
                    result.push_str(&hex);
                }
            } else {
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Extract contiguous runs of base64 characters (A-Za-z0-9+/=) with length >= 16.
fn extract_base64_candidates(text: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    let mut start = None;

    for (i, c) in text.char_indices() {
        if c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=') {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            let run = &text[s..i];
            if run.len() >= 16 {
                candidates.push(run);
            }
        }
    }
    if let Some(s) = start {
        let run = &text[s..];
        if run.len() >= 16 {
            candidates.push(run);
        }
    }

    candidates
}

/// Extract contiguous runs of hex characters (0-9a-fA-F) with even length >= 32.
fn extract_hex_candidates(text: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    let mut start = None;

    for (i, c) in text.char_indices() {
        if c.is_ascii_hexdigit() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            let run = &text[s..i];
            if run.len() >= 32 && run.len().is_multiple_of(2) {
                candidates.push(run);
            }
        }
    }
    if let Some(s) = start {
        let run = &text[s..];
        if run.len() >= 32 && run.len().is_multiple_of(2) {
            candidates.push(run);
        }
    }

    candidates
}

/// Truncate a matched token for safe display (first 12 chars + "...").
fn truncate_match(token: &str) -> String {
    if token.len() <= 16 {
        token.to_string()
    } else {
        format!("{}...", &token[..12])
    }
}

#[cfg(test)]
mod tests {
    use super::{DetectedLeak, LeakEncoding, scan_for_leaks};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

    #[test]
    fn detects_plain_openai_key() {
        let text = "my key is sk-proj_abc123def456ghi789";
        let leaks = scan_for_leaks(text);
        assert!(!leaks.is_empty());
        assert!(
            leaks
                .iter()
                .any(|l| l.kind.contains("OpenAI") && l.encoding == LeakEncoding::Plain)
        );
    }

    #[test]
    fn detects_plain_github_pat() {
        let text = "token: ghp_ABCDEFghijklmnopqrstuvwxyz1234567890";
        let leaks = scan_for_leaks(text);
        assert!(
            leaks
                .iter()
                .any(|l| l.kind.contains("GitHub") && l.encoding == LeakEncoding::Plain)
        );
    }

    #[test]
    fn detects_plain_aws_key() {
        let text = "AKIA1234567890ABCDEFGH";
        let leaks = scan_for_leaks(text);
        assert!(
            leaks
                .iter()
                .any(|l| l.kind.contains("AWS") && l.encoding == LeakEncoding::Plain)
        );
    }

    #[test]
    fn detects_plain_jwt() {
        let text = "token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let leaks = scan_for_leaks(text);
        assert!(
            leaks
                .iter()
                .any(|l| l.kind.contains("JWT") && l.encoding == LeakEncoding::Plain)
        );
    }

    #[test]
    fn detects_url_encoded_key() {
        // sk-proj_abc123def456ghi789 URL-encoded: sk%2Dproj_abc123def456ghi789
        let text = "key=sk%2Dproj_abc123def456ghi789";
        let leaks = scan_for_leaks(text);
        assert!(leaks.iter().any(|l| l.encoding == LeakEncoding::UrlEncoded));
    }

    #[test]
    fn detects_base64_encoded_key() {
        let secret = "sk-proj_abc123def456ghi789jkl";
        let encoded = BASE64_STANDARD.encode(secret);
        let text = format!("encoded: {encoded}");
        let leaks = scan_for_leaks(&text);
        assert!(leaks.iter().any(|l| l.encoding == LeakEncoding::Base64));
    }

    #[test]
    fn detects_hex_encoded_key() {
        let secret = "sk-proj_abc123def456ghi789jkl";
        let encoded = hex::encode(secret);
        let text = format!("hex: {encoded}");
        let leaks = scan_for_leaks(&text);
        assert!(leaks.iter().any(|l| l.encoding == LeakEncoding::Hex));
    }

    #[test]
    fn ignores_text_without_secrets() {
        let text = "This is a normal text with no secrets at all. Just regular content.";
        let leaks = scan_for_leaks(text);
        assert!(leaks.is_empty());
    }

    #[test]
    fn ignores_short_prefix_matches() {
        // Prefix present but token too short to be a real secret.
        let text = "sk-short";
        let leaks = scan_for_leaks(text);
        assert!(leaks.is_empty());
    }

    #[test]
    fn detects_multiple_leaks_in_same_text() {
        let text = "keys: sk-proj_abc123def456ghi789 and ghp_ABCDEFghijklmnopqrstuvwxyz12345";
        let leaks = scan_for_leaks(text);
        assert!(leaks.len() >= 2);
        let kinds: Vec<&str> = leaks.iter().map(|l| l.kind.as_str()).collect();
        assert!(kinds.iter().any(|k| k.contains("OpenAI")));
        assert!(kinds.iter().any(|k| k.contains("GitHub")));
    }

    #[test]
    fn truncates_long_matches() {
        let text = "sk-proj_abc123def456ghi789jklmnopqrstuvwxyz";
        let leaks = scan_for_leaks(text);
        assert!(!leaks.is_empty());
        // Matched value should be truncated.
        assert!(leaks[0].matched.ends_with("..."));
    }

    #[test]
    fn detected_leak_debug_and_clone() {
        let leak = DetectedLeak {
            kind: "test".to_string(),
            matched: "value".to_string(),
            encoding: LeakEncoding::Plain,
        };
        let cloned = leak.clone();
        assert_eq!(leak, cloned);
        // Debug is derivable - just ensure it doesn't panic.
        let _ = format!("{leak:?}");
    }
}
