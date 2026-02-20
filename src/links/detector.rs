use std::collections::HashSet;
use url::Url;

/// Detect HTTP/HTTPS URLs in text. Returns deduplicated URLs in order of appearance.
pub fn detect_urls(text: &str) -> Vec<Url> {
    let mut seen = HashSet::new();
    let mut urls = Vec::new();

    for token in text.split_whitespace() {
        for candidate in extract_candidates(token) {
            if let Some(url) = try_parse_url(&candidate) {
                let key = url.to_string();
                if seen.insert(key) {
                    urls.push(url);
                }
            }
        }
    }

    urls
}

fn extract_candidates(token: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(start) = token.find("](")
        && let Some(end) = token[start..].find(')')
    {
        let url_part = &token[start + 2..start + end];
        candidates.push(url_part.to_string());
        return candidates;
    }

    let stripped = token
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(token);

    let stripped = stripped
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(stripped);

    let stripped = strip_trailing_punctuation(stripped);

    candidates.push(stripped.to_string());
    candidates
}

fn strip_trailing_punctuation(s: &str) -> &str {
    let mut end = s.len();
    let bytes = s.as_bytes();

    while end > 0 {
        let ch = bytes[end - 1];
        if ch == b'.' || ch == b',' || ch == b';' || ch == b'!' || ch == b'?' || ch == b')' {
            end -= 1;
        } else {
            break;
        }
    }

    &s[..end]
}

fn try_parse_url(candidate: &str) -> Option<Url> {
    let url = Url::parse(candidate).ok()?;
    match url.scheme() {
        "http" | "https" => Some(url),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_url() {
        let urls = detect_urls("check https://example.com for info");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].as_str(), "https://example.com/");
    }

    #[test]
    fn multiple_urls() {
        let urls = detect_urls("visit https://a.com and http://b.org today");
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].host_str(), Some("a.com"));
        assert_eq!(urls[1].host_str(), Some("b.org"));
    }

    #[test]
    fn deduplication() {
        let urls = detect_urls("https://example.com and https://example.com again");
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn angle_brackets() {
        let urls = detect_urls("see <https://example.com/path> for details");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].path(), "/path");
    }

    #[test]
    fn trailing_punctuation() {
        let urls = detect_urls("Go to https://example.com/page.");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].path(), "/page");

        let urls = detect_urls("Is it https://example.com?");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].as_str(), "https://example.com/");
    }

    #[test]
    fn parentheses() {
        let urls = detect_urls("(https://example.com/path)");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].path(), "/path");
    }

    #[test]
    fn markdown_link() {
        let urls = detect_urls("click [here](https://example.com/doc) now");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].path(), "/doc");
    }

    #[test]
    fn no_urls() {
        let urls = detect_urls("just some regular text with no links");
        assert!(urls.is_empty());
    }

    #[test]
    fn non_http_schemes_ignored() {
        let urls = detect_urls("ftp://files.example.com mailto:user@example.com");
        assert!(urls.is_empty());
    }

    #[test]
    fn url_with_query_and_fragment() {
        let urls = detect_urls("https://example.com/search?q=test#results");
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].query(), Some("q=test"));
        assert_eq!(urls[0].fragment(), Some("results"));
    }

    #[test]
    fn preserves_order() {
        let urls = detect_urls("https://c.com https://a.com https://b.com");
        assert_eq!(urls[0].host_str(), Some("c.com"));
        assert_eq!(urls[1].host_str(), Some("a.com"));
        assert_eq!(urls[2].host_str(), Some("b.com"));
    }
}
