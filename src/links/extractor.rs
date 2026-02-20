use super::types::{ExtractedContent, LinkConfig};
use anyhow::Result;
use url::Url;

/// Fetch URL content and extract readable text.
pub async fn extract_content(url: &Url, config: &LinkConfig) -> Result<ExtractedContent> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .user_agent("AsteronIris/0.1")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;

    let response = client.get(url.as_str()).send().await?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let is_html = content_type
        .as_deref()
        .is_some_and(|ct| ct.contains("text/html"));

    let body = response.text().await?;

    if is_html {
        Ok(extract_from_html(
            url.as_str(),
            &body,
            config.max_content_chars,
        ))
    } else {
        let truncated = truncate_text(&body, config.max_content_chars);
        Ok(ExtractedContent {
            url: url.to_string(),
            title: None,
            text: truncated,
            content_type,
        })
    }
}

fn extract_from_html(url: &str, html: &str, max_chars: usize) -> ExtractedContent {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    let title = Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|t| !t.is_empty());

    let content = extract_element_text(&document, "article")
        .or_else(|| extract_element_text(&document, "main"))
        .or_else(|| extract_element_text(&document, "body"))
        .unwrap_or_default();

    let truncated = truncate_text(&content, max_chars);

    ExtractedContent {
        url: url.to_string(),
        title,
        text: truncated,
        content_type: Some("text/html".to_string()),
    }
}

fn extract_element_text(document: &scraper::Html, selector: &str) -> Option<String> {
    let sel = scraper::Selector::parse(selector).ok()?;
    let element = document.select(&sel).next()?;
    let text: String = element.text().collect::<Vec<_>>().join(" ");
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

/// Enrich a user message with extracted link content.
pub async fn enrich_message_with_links(message: &str, config: &LinkConfig) -> String {
    if !config.enabled {
        return message.to_string();
    }

    let urls = super::detector::detect_urls(message);
    if urls.is_empty() {
        return message.to_string();
    }

    let urls_to_process: Vec<_> = urls
        .into_iter()
        .take(config.max_links_per_message)
        .collect();
    let mut context_parts: Vec<String> = Vec::new();

    for url in &urls_to_process {
        match extract_content(url, config).await {
            Ok(content) => {
                let title_part = content.title.as_deref().unwrap_or("Untitled");
                context_parts.push(format!(
                    "[Link: {title_part}]\nURL: {}\n{}\n",
                    content.url, content.text
                ));
            }
            Err(e) => {
                tracing::debug!(url = %url, error = %e, "link extraction failed");
            }
        }
    }

    if context_parts.is_empty() {
        return message.to_string();
    }

    format!(
        "{message}\n\n---\nExtracted link content:\n{}\n---",
        context_parts.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_within_limit() {
        let result = truncate_text("hello", 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn truncate_exceeds_limit() {
        let result = truncate_text("hello world", 5);
        assert_eq!(result, "hello...");
    }

    #[test]
    fn truncate_unicode() {
        let result = truncate_text("abcde", 3);
        assert_eq!(result, "abc...");
    }

    #[test]
    fn extract_html_title_and_body() {
        let html =
            r#"<html><head><title>Test Page</title></head><body><p>Hello world</p></body></html>"#;
        let result = extract_from_html("https://example.com", html, 2000);
        assert_eq!(result.title.as_deref(), Some("Test Page"));
        assert!(result.text.contains("Hello world"));
    }

    #[test]
    fn extract_html_article_preferred() {
        let html = r#"<html><body><nav>Nav stuff</nav><article><p>Article content</p></article></body></html>"#;
        let result = extract_from_html("https://example.com", html, 2000);
        assert_eq!(result.text, "Article content");
    }

    #[test]
    fn extract_html_falls_back_to_body() {
        let html = r#"<html><body><p>Body content here</p></body></html>"#;
        let result = extract_from_html("https://example.com", html, 2000);
        assert!(result.text.contains("Body content here"));
    }

    #[test]
    fn extract_html_truncates() {
        let html = r#"<html><body><p>A long paragraph of text</p></body></html>"#;
        let result = extract_from_html("https://example.com", html, 10);
        assert!(result.text.ends_with("..."));
        assert!(result.text.chars().count() <= 13); // 10 + "..."
    }

    #[tokio::test]
    async fn enrich_no_urls() {
        let config = LinkConfig::default();
        let result = enrich_message_with_links("just text", &config).await;
        assert_eq!(result, "just text");
    }

    #[tokio::test]
    async fn enrich_disabled() {
        let config = LinkConfig {
            enabled: false,
            ..LinkConfig::default()
        };
        let result = enrich_message_with_links("check https://example.com", &config).await;
        assert_eq!(result, "check https://example.com");
    }
}
