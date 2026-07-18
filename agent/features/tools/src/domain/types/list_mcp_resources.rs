//! Typed result for the `list_mcp_resources` tool (non-core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed result returned by the `list_mcp_resources` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ListMcpResourcesResult {
    pub resources: Vec<Value>,
}
