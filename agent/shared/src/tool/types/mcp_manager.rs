//! Typed result for the `mcp_manager` tool (non-core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `mcp_manager` tool (lifecycle operations on
/// connected MCP servers).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct McpManagerResult {
    pub action: String,
    pub status: String,
}