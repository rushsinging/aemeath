//! Typed result for the `web_search` tool (non-core tool).

use super::support::SearchResult;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `web_search` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct WebSearchResult {
    pub results: Vec<SearchResult>,
}