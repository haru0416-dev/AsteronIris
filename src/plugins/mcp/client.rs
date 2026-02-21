pub mod connection {
    pub use crate::plugins::mcp::client_connection::*;
}

pub mod manager {
    pub use crate::plugins::mcp::client_manager::*;
}

pub mod proxy_tool {
    pub use crate::plugins::mcp::client_proxy_tool::*;
}

#[allow(unused_imports)]
pub use connection::McpConnection;
#[allow(unused_imports)]
pub use manager::{McpManager, create_mcp_tools};
#[allow(unused_imports)]
pub use proxy_tool::McpToolProxy;
