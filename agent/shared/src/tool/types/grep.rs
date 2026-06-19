//! Typed result for the `grep` tool (issue #273 core tool).

use super::support::Match;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `grep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GrepResult {
    pub matches: Vec<Match>,
    pub total_matches: u64,
    pub query: String,
}

/// Typed input for the `grep` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GrepInput {
    /// Regex pattern to search for
    pub pattern: String,
    /// File or directory to search in (defaults to cwd)
    pub path: Option<String>,
    /// File glob filter (e.g. "*.rs")
    pub glob: Option<String>,
}
