use crate::config::Config;
use crate::memory::Memory;
use crate::tools::middleware::default_middleware_chain;
use crate::tools::{self, ToolRegistry};
use std::sync::Arc;

/// Build the standard tool registry for integration-layer session turns.
///
/// Session code (`session.rs`) delegates here so that both the main-session
/// path and the integration-test path share one tool-initialisation routine.
pub(super) fn init_tools(_config: &Config, mem: &Arc<dyn Memory>) -> Arc<ToolRegistry> {
    let tools = tools::all_tools(Arc::clone(mem));
    let middleware = default_middleware_chain();
    let mut registry = ToolRegistry::new(middleware);
    for tool in tools {
        registry.register(tool);
    }
    Arc::new(registry)
}
