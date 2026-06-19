//! Typed result for the `web_search` tool (non-core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed result returned by the `web_search` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebSearchResult {
    pub results: Vec<Value>,
}
