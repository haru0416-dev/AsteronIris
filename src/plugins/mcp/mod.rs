//! MCP (Model Context Protocol) subsystem.
//!
//! Provides client connections to external MCP servers and
//! optional server mode for exposing tools via MCP.

pub mod bridge;
pub(crate) mod client_connection;
pub(crate) mod client_manager;
pub(crate) mod client_proxy_tool;
pub mod content;
pub mod server;

#[allow(unused_imports)]
pub use client_connection::McpConnection;
#[allow(unused_imports)]
pub use client_manager::{McpManager, create_mcp_tools};
#[allow(unused_imports)]
pub use client_proxy_tool::McpToolProxy;
