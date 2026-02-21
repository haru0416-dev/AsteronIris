//! Browser automation tool using Vercel's agent-browser CLI
//!
//! This tool provides AI-optimized web browsing capabilities via the agent-browser CLI.
//! It supports semantic element selection, accessibility snapshots, and JSON output
//! for efficient LLM integration.

mod domain;
mod tool_impl;
mod types;

pub use tool_impl::BrowserTool;
#[allow(unused_imports)]
pub use types::BrowserAction;

#[cfg(test)]
mod tests;
