//! Typed result for the `tool_search` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `tool_search` tool.
///
/// `tools` lists the names of all tools whose name/description matched
/// the search query, in stable sort order.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolSearchResult {
    pub tools: Vec<String>,
}

/// Typed input for the `tool_search` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolSearchInput {
    /// Search query - tool name or functionality keyword
    pub query: String,
}
