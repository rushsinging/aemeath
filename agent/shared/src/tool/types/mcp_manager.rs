//! Typed result for the `mcp_manager` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `mcp_manager` tool (lifecycle operations on
/// connected MCP servers).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpManagerResult {
    pub name: String,
    pub action: String,
}
