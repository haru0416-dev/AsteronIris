use std::borrow::Cow;

const MAX_API_ERROR_CHARS: usize = 200;

fn is_secret_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '+' | '/' | '=')
}

fn token_end(input: &str, from: usize) -> usize {
    let mut end = from;
    for (i, c) in input[from..].char_indices() {
        if is_secret_char(c) {
            end = from + i + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn scrub_after_marker(scrubbed: &mut String, marker: &str) -> bool {
    let mut modified = false;
    let mut search_from = 0;
    loop {
        let Some(rel) = scrubbed[search_from..].find(marker) else {
            break;
        };

        let start = search_from + rel;
        let content_start = start + marker.len();
        let end = token_end(scrubbed, content_start);

        // Skip bare markers without a token value.
        if end == content_start {
            search_from = content_start;
            continue;
        }

        scrubbed.replace_range(start..end, "[REDACTED]");
        modified = true;
        search_from = start + "[REDACTED]".len();
    }

    modified
}

fn needs_scrubbing(input: &str) -> bool {
    const ALL_PATTERNS: [&str; 25] = [
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxs-",
        "xoxa-",
        "xapp-",
        "ghp_",
        "github_pat_",
        "hf_",
        "glpat-",
        "ya29.",
        "AIza",
        "Authorization: Bearer ",
        "authorization: bearer ",
        "\"authorization\":\"Bearer ",
        "\"authorization\":\"bearer ",
        "api_key=",
        "access_token=",
        "refresh_token=",
        "id_token=",
        "\"api_key\":\"",
        "\"access_token\":\"",
        "\"refresh_token\":\"",
        "\"id_token\":\"",
        "\"token\":\"",
    ];

    ALL_PATTERNS.iter().any(|pattern| input.contains(pattern))
}

/// Scrub known secret-like token patterns from provider error strings.
///
/// Redacts provider keys and tokens in common forms:
/// - Prefix tokens: `sk-`, `xoxb-`, `ghp_`, etc.
/// - Header/query/json markers: `Authorization: Bearer ...`, `api_key=...`, `"access_token":"..."`
pub fn scrub_secret_patterns(input: &str) -> Cow<'_, str> {
    const PREFIX_PATTERNS: [&str; 12] = [
        "sk-",
        "xoxb-",
        "xoxp-",
        "xoxs-",
        "xoxa-",
        "xapp-",
        "ghp_",
        "github_pat_",
        "hf_",
        "glpat-",
        "ya29.",
        "AIza",
    ];

    const MARKER_PATTERNS: [&str; 13] = [
        "Authorization: Bearer ",
        "authorization: bearer ",
        "\"authorization\":\"Bearer ",
        "\"authorization\":\"bearer ",
        "api_key=",
        "access_token=",
        "refresh_token=",
        "id_token=",
        "\"api_key\":\"",
        "\"access_token\":\"",
        "\"refresh_token\":\"",
        "\"id_token\":\"",
        "\"token\":\"",
    ];

    if !needs_scrubbing(input) {
        return Cow::Borrowed(input);
    }

    let mut scrubbed = input.to_string();

    for pattern in PREFIX_PATTERNS {
        scrub_after_marker(&mut scrubbed, pattern);
    }

    for marker in MARKER_PATTERNS {
        scrub_after_marker(&mut scrubbed, marker);
    }

    Cow::Owned(scrubbed)
}

/// Sanitize API error text by scrubbing secrets and truncating length.
pub fn sanitize_api_error(input: &str) -> String {
    let scrubbed = scrub_secret_patterns(input);

    if scrubbed.chars().count() <= MAX_API_ERROR_CHARS {
        return scrubbed.into_owned();
    }

    let scrubbed = scrubbed.as_ref();
    let mut end = MAX_API_ERROR_CHARS;
    while end > 0 && !scrubbed.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}...", &scrubbed[..end])
}

/// Build a sanitized provider error from a failed HTTP response.
pub async fn api_error(provider: &str, response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read provider error body>".to_string());
    let sanitized = sanitize_api_error(&body);
    anyhow::anyhow!("{provider} API error ({status}): {sanitized}")
}
