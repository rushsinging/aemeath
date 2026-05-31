//! Gateway/OHS for tool catalog and registration.
//!
//! Migration-period exports delegate to the existing registry and registration
//! orchestration without moving execution logic.

pub use crate::business::bash::is_readonly_command;
pub use crate::business::mcp_manager::McpConnectionManager;
pub use crate::business::mcp_tool::McpTool;
pub use crate::core::registry::{
    register_all_tools, register_all_tools_except_agent, register_subagent_tools,
};
pub use crate::core::tool_registry::ToolRegistry;

/// Published name for the tool catalog gateway.
pub type ToolCatalog = ToolRegistry;
