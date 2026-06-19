//! Typed result for the `list_mcp_resources` tool (non-core tool).

use super::support::McpResource;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `list_mcp_resources` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct ListMcpResourcesResult {
    pub resources: Vec<McpResource>,
}