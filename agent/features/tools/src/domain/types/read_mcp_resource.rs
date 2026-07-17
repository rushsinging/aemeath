//! Typed result for the `read_mcp_resource` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `read_mcp_resource` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReadMcpResourceResult {
    pub uri: String,
}
