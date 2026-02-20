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
