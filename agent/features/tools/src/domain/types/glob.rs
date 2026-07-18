//! Typed result for the `glob` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `glob` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GlobResult {
    pub files: Vec<String>,
    pub count: u64,
}

/// Typed input for the `glob` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GlobInput {
    /// Glob pattern (e.g. "**/*.rs")
    pub pattern: String,
    /// Directory to search in (defaults to cwd)
    pub path: Option<String>,
}
