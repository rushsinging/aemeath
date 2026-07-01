//! Typed result for the `web_fetch` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `web_fetch` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WebFetchResult {
    pub url: String,
    pub title: String,
    pub content: String,
    pub truncated: bool,
    pub links: Vec<String>,
}

/// Typed input for the `web_fetch` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct WebFetchInput {
    /// The URL to fetch
    pub url: String,
    /// Timeout in milliseconds (default 30000)
    pub timeout: Option<u64>,
}
