//! Typed result for the `list_mcp_resources` tool (non-core tool).

use super::support::McpResource;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `list_mcp_resources` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {resources: array}
pub struct ListMcpResourcesResult {
    pub resources: Vec<McpResource>,
}