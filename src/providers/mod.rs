pub mod anthropic;
mod anthropic_types;
pub mod compatible;
pub mod factory;
pub mod fallback_tools;
pub mod gemini;
mod gemini_types;
pub mod http_client;
pub mod oauth_recovery;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod reliable;
pub mod response;
pub mod scrub;
pub mod sse;
pub mod streaming;
pub mod tool_convert;
pub mod traits;

#[allow(unused_imports)]
pub use factory::{
    create_provider, create_provider_with_oauth_recovery, create_resilient_provider,
    create_resilient_provider_with_oauth_recovery, create_resilient_provider_with_resolver,
};
pub use http_client::{build_provider_client, build_provider_client_with_timeout};
#[allow(unused_imports)]
pub use response::{
    ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
};
#[allow(unused_imports)]
pub use scrub::{api_error, sanitize_api_error, scrub_secret_patterns};
#[allow(unused_imports)]
pub use streaming::{
    ChannelStreamSink, CliStreamSink, NullStreamSink, ProviderChatRequest, ProviderStream,
    StreamCollector, StreamEvent, StreamSink, StreamingSecretScrubber,
};
pub use traits::Provider;

#[cfg(test)]
mod tests;
