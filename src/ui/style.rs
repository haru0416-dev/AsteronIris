use console::style;
use std::fmt::Display;

/// Green bold — success checkmarks, confirmations
pub fn success<D: Display>(text: D) -> String {
    style(text).green().bold().to_string()
}

/// White bold — section headers, titles
pub fn header<D: Display>(text: D) -> String {
    style(text).white().bold().to_string()
}

/// Dim — subtitles, secondary text, decorative lines
pub fn dim<D: Display>(text: D) -> String {
    style(text).dim().to_string()
}

/// Yellow — shell commands, code snippets, warnings
pub fn yellow<D: Display>(text: D) -> String {
    style(text).yellow().to_string()
}

/// Green — confirmed values, paths, names
pub fn value<D: Display>(text: D) -> String {
    style(text).green().to_string()
}

/// Cyan bold — step numbers, bullet points
pub fn accent<D: Display>(text: D) -> String {
    style(text).cyan().bold().to_string()
}

/// Cyan — secondary accent, field labels
pub fn cyan<D: Display>(text: D) -> String {
    style(text).cyan().to_string()
}

/// Cyan underlined — URLs, links
pub fn url<D: Display>(text: D) -> String {
    style(text).cyan().underlined().to_string()
}

/// Green dim — secondary confirmed values
pub fn dim_value<D: Display>(text: D) -> String {
    style(text).green().dim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_and_header_preserve_text_content() {
        let input = "operation-complete";
        assert!(success(input).contains(input));
        assert!(header(input).contains(input));
    }

    #[test]
    fn styling_helpers_accept_non_string_display_types() {
        let value_text = value(1234_u32);
        let retries_text = dim_value(0.125_f32);

        assert!(value_text.contains("1234"));
        assert!(retries_text.contains("0.125"));
    }

    #[test]
    fn accent_and_url_include_original_text() {
        let label = "step-1";
        let link = "https://example.test";

        assert!(accent(label).contains(label));
        assert!(url(link).contains(link));
        assert!(cyan("field").contains("field"));
        assert!(yellow("cmd").contains("cmd"));
        assert!(dim("hint").contains("hint"));
    }
}
