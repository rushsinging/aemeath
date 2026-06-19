//! Typed result for the `read_mcp_resource` tool (non-core tool).

use super::support::ResourceContent;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `read_mcp_resource` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct ReadMcpResourceResult {
    pub contents: ResourceContent,
}