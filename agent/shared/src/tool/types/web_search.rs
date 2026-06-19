//! Typed result for the `web_search` tool (non-core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed result returned by the `web_search` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebSearchResult {
    pub results: Vec<Value>,
}

/// Typed input for the `web_search` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WebSearchInput {
    /// The search query
    pub query: String,
    /// Maximum number of results to return (default 5, max 10)
    pub limit: Option<u64>,
}
