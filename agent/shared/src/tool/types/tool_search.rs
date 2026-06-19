//! Typed result for the `tool_search` tool (non-core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `tool_search` tool.
///
/// `tools` lists the names of all tools whose name/description matched
/// the search query, in stable sort order.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct ToolSearchResult {
    pub tools: Vec<String>,
}