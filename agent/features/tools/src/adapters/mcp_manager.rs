//! MCP Connection Manager
//!
//! Manages connections to multiple MCP servers, providing:
//! - Server configuration loading
//! - Connection lifecycle management
//! - Tool discovery and registration
//! - Resource discovery
//! - Reconnection handling

pub mod config;
pub mod connection;
pub mod diff;
pub mod wrapper;

pub use crate::domain::types::mcp_manager::McpManagerResult;

pub use config::{ConnectionState, McpManagerConfig, McpServerConnection};
pub use connection::McpConnectionManager;
pub use diff::{diff_tools, qualified_tool_name, removed_qualified_tool_names, ToolListDiff};

#[cfg(test)]
mod tests;
