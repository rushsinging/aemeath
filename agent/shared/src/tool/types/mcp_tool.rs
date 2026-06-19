//! Typed result for the `mcp_tool` tool (non-core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed result returned by the `mcp_tool` tool (call an arbitrary MCP tool).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {tool_name: string, provider: string, content: string}
pub struct McpToolResult {
    pub server: String,
    pub tool: String,
    pub output: Value,
}