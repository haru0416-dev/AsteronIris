pub mod auth {
    pub use crate::transport::gateway::openai_compat_auth::*;
}

pub mod handler {
    pub use crate::transport::gateway::openai_compat_handler::*;
}

pub mod streaming {
    pub use crate::transport::gateway::openai_compat_streaming::*;
}

pub mod types {
    pub use crate::transport::gateway::openai_compat_types::*;
}

pub use handler::handle_chat_completions;
