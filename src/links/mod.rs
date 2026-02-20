pub mod detector;
pub mod extractor;
pub mod types;

pub use detector::detect_urls;
pub use extractor::enrich_message_with_links;
pub use types::{ExtractedContent, LinkConfig};
