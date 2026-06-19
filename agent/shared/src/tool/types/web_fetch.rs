//! Typed result for the `web_fetch` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `web_fetch` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct WebFetchResult {
    pub url: String,
    pub byte_count: u64,
    pub char_count: u64,
    pub truncated: bool,
}