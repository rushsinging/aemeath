//! Typed result for the `mcp_tool` tool (non-core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;
use serde_json::Value;

/// Typed result returned by the `mcp_tool` tool (call an arbitrary MCP tool).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct McpToolResult {
    pub server: String,
    pub tool: String,
    pub output: Value,
}