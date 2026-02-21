//! MCP (Model Context Protocol) subsystem.
//!
//! Provides client connections to external MCP servers and
//! optional server mode for exposing tools via MCP.

pub mod bridge;
pub mod client;
pub mod content;
pub mod server;

#[allow(unused_imports)]
pub use client::manager::McpManager;
#[allow(unused_imports)]
pub use client::proxy_tool::McpToolProxy;
