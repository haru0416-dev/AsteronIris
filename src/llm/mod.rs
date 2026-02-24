// ── Infrastructure ───────────────────────────────────────────────────────────
pub mod coercion;
pub mod cooldown;
pub mod fallback_tools;
pub mod http_client;
pub mod leak_detect;
pub mod scrub;
pub mod sse;
pub mod streaming;
pub mod tool_convert;
pub mod traits;
pub mod types;

// ── Decorator layers ────────────────────────────────────────────────────────
pub mod factory;
pub mod manager;
pub mod oauth_recovery;
pub mod reliable;

// ── Provider implementations ────────────────────────────────────────────────
pub mod anthropic;
pub mod compatible;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod openrouter;

// ── Infrastructure re-exports ───────────────────────────────────────────────
pub use coercion::{coerce_arguments, coerce_value};
pub use cooldown::CooldownTracker;
pub use fallback_tools::{
    ExtractedToolCall, augment_system_prompt_with_tools, build_fallback_response,
    extract_tool_calls,
};
pub use http_client::{build_provider_client, build_provider_client_with_timeout};
pub use leak_detect::{DetectedLeak, LeakEncoding, scan_for_leaks};
pub use scrub::{api_error, sanitize_api_error, scrub_secret_patterns};
pub use sse::{SseBuffer, parse_data_lines, parse_data_lines_without_done, parse_event_data_pairs};
pub use streaming::{
    ChannelStreamSink, CliStreamSink, NullStreamSink, ProviderStream, StreamCollector, StreamEvent,
    StreamSink,
};
pub use tool_convert::{ToolFields, map_tools_optional};
pub use traits::{Provider, ProviderCapabilities, messages_to_text};
pub use types::{
    ContentBlock, ImageSource, MessageRole, ProviderMessage, ProviderResponse, StopReason,
};

// ── Provider + factory re-exports ───────────────────────────────────────────
pub use anthropic::AnthropicProvider;
pub use compatible::{AuthStyle, OpenAiCompatibleProvider};
#[allow(unused_imports)]
pub use factory::{
    create_provider, create_provider_with_oauth_recovery, create_resilient_provider,
    create_resilient_provider_with_oauth_recovery, create_resilient_provider_with_resolver,
};
pub use gemini::GeminiProvider;
pub use manager::LlmManager;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use openrouter::OpenRouterProvider;
