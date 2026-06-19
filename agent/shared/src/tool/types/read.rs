//! Typed result for the `read` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `read` tool.
///
/// Fields cover the `(N lines)` / `(N bytes)` / offset / limit metadata that
/// the TUI header needs, plus the truncated flag for streaming reads.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {content: string, file_path: string, line_count: integer, start_line: integer, total_lines: integer}
pub struct ReadResult {
    pub content: String,
    pub file_path: String,
    pub line_count: u64,
    pub start_line: u64,
    pub total_lines: u64,
}