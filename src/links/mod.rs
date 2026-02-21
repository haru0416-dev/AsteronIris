pub mod detector;
#[cfg(feature = "link-extraction")]
pub mod extractor;
pub mod types;

pub use detector::detect_urls;
#[cfg(feature = "link-extraction")]
pub use extractor::enrich_message_with_links;
pub use types::{ExtractedContent, LinkConfig};
