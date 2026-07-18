//! Typed result for the `read` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `read` tool.
///
/// Fields cover the `(N lines)` / `(N bytes)` / offset / limit metadata that
/// the TUI header needs, plus the truncated flag for streaming reads.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ReadResult {
    pub content: String,
    pub file_path: String,
    pub line_count: u64,
    pub start_line: u64,
    pub total_lines: u64,
}

/// Typed input for the `read` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ReadInput {
    /// Absolute path to the file to read
    pub file_path: String,
    /// Line number to start reading from (0-based, default: 0)
    pub offset: Option<u64>,
    /// Maximum number of lines to read (default: 2000)
    pub limit: Option<u64>,
}
