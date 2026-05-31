//! Published language for the tools feature.
//!
//! This module exposes tool-domain DTOs and shared-kernel tool types without
//! exposing tool execution internals.

pub use share::tool::{
    AgentRunRequest, AgentToolCallProgress, ImageData, Tool, ToolChangeSet, ToolContext, ToolResult,
};

pub use crate::business::mcp::{McpServerConfig, McpToolDef, McpTransportKind};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    #[test]
    fn test_contract_reexports_tool_execution_types() {
        assert_send_sync::<dyn Tool>();
        let _ = std::mem::size_of::<ToolContext>();
        let _ = std::mem::size_of::<ToolResult>();
        let _ = std::mem::size_of::<ToolChangeSet>();
        let _ = std::mem::size_of::<AgentRunRequest<'_>>();
    }
}
