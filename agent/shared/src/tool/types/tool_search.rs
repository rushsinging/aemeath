//! Typed result for the `tool_search` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `tool_search` tool.
///
/// `tools` lists the names of all tools whose name/description matched
/// the search query, in stable sort order.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {tools: array}
pub struct ToolSearchResult {
    pub tools: Vec<String>,
}
