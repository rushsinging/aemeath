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
///
/// `timeout` **MUST NOT** 在此声明：框架级通用参数，catalog 注入发布、
/// 执行前框架摘除，工具只读 `ctx.effective_timeout_secs()`。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct WebFetchInput {
    /// The URL to fetch
    pub url: String,
}
