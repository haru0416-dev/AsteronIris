pub mod anthropic;
pub mod compatible;
pub mod factory;
pub mod fallback_tools;
pub mod gemini;
pub mod oauth_recovery;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod reliable;
pub mod response;
pub mod scrub;
pub mod traits;

#[allow(unused_imports)]
pub use factory::{
    create_provider, create_provider_with_oauth_recovery, create_resilient_provider,
    create_resilient_provider_with_oauth_recovery, create_resilient_provider_with_resolver,
};
#[allow(unused_imports)]
pub use response::{ContentBlock, MessageRole, ProviderMessage, ProviderResponse, StopReason};
#[allow(unused_imports)]
pub use scrub::{api_error, sanitize_api_error, scrub_secret_patterns};
pub use traits::Provider;

#[cfg(test)]
mod tests;
