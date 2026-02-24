use super::file_read::FileReadTool;
use super::file_write::FileWriteTool;
use super::memory::{MemoryForgetTool, MemoryGovernanceTool, MemoryRecallTool, MemoryStoreTool};
use super::shell::ShellTool;
use super::traits::Tool;
use crate::memory::Memory;
use std::sync::Arc;

/// Create the default set of core tools (shell, `file_read`, `file_write`).
pub fn default_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ShellTool::new()),
        Box::new(FileReadTool::new()),
        Box::new(FileWriteTool::new()),
    ]
}

/// Create the full tool set including memory tools.
///
/// Additional tools (browser, composio, MCP, etc.) can be appended by callers.
pub fn all_tools(memory: Arc<dyn Memory>) -> Vec<Box<dyn Tool>> {
    let mut tools: Vec<Box<dyn Tool>> = default_tools();

    tools.push(Box::new(MemoryStoreTool::new(Arc::clone(&memory))));
    tools.push(Box::new(MemoryRecallTool::new(Arc::clone(&memory))));
    tools.push(Box::new(MemoryForgetTool::new(Arc::clone(&memory))));
    tools.push(Box::new(MemoryGovernanceTool::new(memory)));

    tools
}

/// Generate tool descriptions for system prompts.
///
/// Returns a vector of (`tool_name`, description) tuples.
pub fn tool_descriptions() -> Vec<(String, String)> {
    vec![
        (
            "shell".to_string(),
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.".to_string(),
        ),
        (
            "file_read".to_string(),
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.".to_string(),
        ),
        (
            "file_write".to_string(),
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.".to_string(),
        ),
        (
            "memory_store".to_string(),
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.".to_string(),
        ),
        (
            "memory_recall".to_string(),
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.".to_string(),
        ),
        (
            "memory_forget".to_string(),
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.".to_string(),
        ),
        (
            "memory_governance".to_string(),
            "Run governance inspect/export/delete actions on memory with audit logging.".to_string(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tools_returns_three_core_tools() {
        let tools = default_tools();
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
    }

    #[test]
    fn tool_descriptions_covers_core_tools() {
        let descs = tool_descriptions();
        let names: Vec<&str> = descs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"memory_store"));
        assert!(names.contains(&"memory_recall"));
        assert!(names.contains(&"memory_forget"));
        assert!(names.contains(&"memory_governance"));
    }
}
