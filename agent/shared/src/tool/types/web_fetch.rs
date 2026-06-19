//! Typed result for the `web_fetch` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `web_fetch` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {url: string, title: string, content: string, truncated: boolean}
pub struct WebFetchResult {
    pub url: String,
    pub title: String,
    pub content: String,
    pub truncated: bool,
}