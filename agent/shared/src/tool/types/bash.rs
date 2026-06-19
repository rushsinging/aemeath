//! Typed result for the `bash` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `bash` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {stdout: string, stderr: string, exit_code: integer, signal: integer?}
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub signal: Option<i32>,
}
