pub mod browser;
pub mod browser_open;
mod common;
pub mod composio;
pub mod delegate;
pub mod factory;
pub mod file_read;
pub mod file_write;
pub mod memory;
pub mod middleware;
pub mod registry;
pub mod shell;
pub mod subagent;
#[cfg(feature = "taste")]
pub mod taste;
pub mod traits;

pub use browser::BrowserTool;
pub use browser_open::BrowserOpenTool;
pub use composio::ComposioTool;
pub use delegate::DelegateTool;
#[allow(unused_imports)]
pub use factory::{all_tools, default_action_operator, default_tools, tool_descriptions};
pub use file_read::FileReadTool;
pub use file_write::FileWriteTool;
pub use memory::{MemoryForgetTool, MemoryGovernanceTool, MemoryRecallTool, MemoryStoreTool};
#[allow(unused_imports)]
pub use middleware::{
    ExecutionContext, MiddlewareDecision, ToolMiddleware, default_middleware_chain,
};
#[allow(unused_imports)]
pub use registry::ToolRegistry;
pub use shell::ShellTool;
pub use subagent::{SubagentCancelTool, SubagentOutputTool, SubagentSpawnTool};
#[cfg(feature = "taste")]
pub use taste::{TasteCompareTool, TasteEvaluateTool};
pub use traits::Tool;
#[allow(unused_imports)]
pub use traits::{
    ActionIntent, ActionOperator, ActionResult, NoopOperator, OutputAttachment, ToolResult,
    ToolSpec,
};

#[cfg(any(feature = "mcp", test))]
#[allow(unused_imports)]
pub(crate) use factory::append_dynamic_tool_descriptions;

#[cfg(test)]
mod tests;
