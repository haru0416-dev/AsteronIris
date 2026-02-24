use thiserror::Error;

// ─── Top-level error hierarchy ───────────────────────────────────────────────

/// Structured error hierarchy for `AsteronIris`.
///
/// Each subsystem defines its own error variant. Library callers can match on
/// these to decide recovery strategy; internal code continues to use
/// `anyhow::Result` for ad-hoc context chains.
#[derive(Debug, Error)]
pub enum IrisError {
    // ── Config ───────────────────────────────────────────────────────────
    #[error("config: {0}")]
    Config(#[from] ConfigError),

    // ── LLM / Provider ──────────────────────────────────────────────────
    #[error("llm: {0}")]
    Llm(#[from] LlmError),

    // ── Memory ──────────────────────────────────────────────────────────
    #[error("memory: {0}")]
    Memory(#[from] MemoryError),

    // ── Session ─────────────────────────────────────────────────────────
    #[error("session: {0}")]
    Session(#[from] SessionError),

    // ── Tools ───────────────────────────────────────────────────────────
    #[error("tool: {0}")]
    Tool(#[from] ToolError),

    // ── Security / Policy ───────────────────────────────────────────────
    #[error("security: {0}")]
    Security(#[from] SecurityError),

    // ── Process model ───────────────────────────────────────────────────
    #[error("process: {0}")]
    Process(#[from] ProcessError),

    // ── Transport / Channel ─────────────────────────────────────────────
    #[error("transport: {0}")]
    Transport(#[from] TransportError),

    // ── Prompt / Template ───────────────────────────────────────────────
    #[error("prompt: {0}")]
    Prompt(#[from] PromptError),

    // ── Generic fallthrough (wraps anyhow for interop) ──────────────────
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// ─── Config errors ───────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load config: {0}")]
    Load(String),

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("hot-reload failed: {0}")]
    HotReload(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

// ─── LLM / Provider errors ──────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("provider {provider} request failed: {message}")]
    Request { provider: String, message: String },

    #[error("provider {provider} rate-limited (retry after {retry_after_secs}s)")]
    RateLimited {
        provider: String,
        retry_after_secs: u64,
    },

    #[error("provider {provider} authentication failed")]
    Auth { provider: String },

    #[error("model {model} not found on provider {provider}")]
    ModelNotFound { provider: String, model: String },

    #[error("streaming error: {0}")]
    Streaming(String),

    #[error("secret leak detected: {0}")]
    LeakDetected(String),

    #[error("cooldown active for provider {provider} until {until}")]
    Cooldown { provider: String, until: String },
}

// ─── Memory errors ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("query failed: {0}")]
    Query(String),

    #[error("embedding failed: {0}")]
    Embedding(String),

    #[error("schema migration failed: {0}")]
    Migration(String),

    #[error("backend not available: {0}")]
    BackendUnavailable(String),

    #[error("sqlx: {0}")]
    Sqlx(String),
}

// ─── Session errors ─────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session not found: {0}")]
    NotFound(String),

    #[error("compaction failed: {0}")]
    Compaction(String),

    #[error("store: {0}")]
    Store(String),
}

// ─── Tool errors ────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool {name} not found")]
    NotFound { name: String },

    #[error("tool {name} execution failed: {message}")]
    Execution { name: String, message: String },

    #[error("tool {name} denied by policy: {reason}")]
    PolicyDenied { name: String, reason: String },

    #[error("isolation error: {0}")]
    Isolation(String),
}

// ─── Security errors ────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("action denied: {0}")]
    Denied(String),

    #[error("authentication required: {0}")]
    AuthRequired(String),

    #[error("secret store: {0}")]
    SecretStore(String),

    #[error("pairing: {0}")]
    Pairing(String),
}

// ─── Process model errors ───────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("branch {entity_id} failed: {message}")]
    Branch { entity_id: String, message: String },

    #[error("worker error: {0}")]
    Worker(String),

    #[error("compactor error: {0}")]
    Compactor(String),

    #[error("cortex error: {0}")]
    Cortex(String),
}

// ─── Transport errors ───────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("channel {channel} connection failed: {message}")]
    Connection { channel: String, message: String },

    #[error("channel {channel} send failed: {message}")]
    Send { channel: String, message: String },

    #[error("gateway: {0}")]
    Gateway(String),
}

// ─── Prompt / Template errors ───────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PromptError {
    #[error("template render failed: {0}")]
    Render(String),

    #[error("template not found: {0}")]
    NotFound(String),
}

// ─── Convenience re-exports ─────────────────────────────────────────────────

/// Shorthand result type for the crate.
pub type Result<T> = std::result::Result<T, IrisError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_displays_correctly() {
        let err = IrisError::Config(ConfigError::Validation("bad temp".into()));
        assert!(err.to_string().contains("validation failed"));
    }

    #[test]
    fn llm_rate_limited_displays_retry() {
        let err = IrisError::Llm(LlmError::RateLimited {
            provider: "anthropic".into(),
            retry_after_secs: 30,
        });
        assert!(err.to_string().contains("30s"));
    }

    #[test]
    fn anyhow_interop() {
        let anyhow_err = anyhow::anyhow!("something went wrong");
        let iris_err: IrisError = anyhow_err.into();
        assert!(iris_err.to_string().contains("something went wrong"));
    }

    #[test]
    fn memory_error_displays_correctly() {
        let err = IrisError::Memory(MemoryError::BackendUnavailable("lancedb".into()));
        assert!(err.to_string().contains("lancedb"));
    }

    #[test]
    fn tool_policy_denied_displays_correctly() {
        let err = IrisError::Tool(ToolError::PolicyDenied {
            name: "shell".into(),
            reason: "read_only mode".into(),
        });
        assert!(err.to_string().contains("shell"));
        assert!(err.to_string().contains("read_only mode"));
    }
}
