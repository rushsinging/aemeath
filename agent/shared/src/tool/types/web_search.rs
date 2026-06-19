//! Typed result for the `web_search` tool (non-core tool).

use super::support::SearchResult;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `web_search` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {results: array}
pub struct WebSearchResult {
    pub results: Vec<SearchResult>,
}