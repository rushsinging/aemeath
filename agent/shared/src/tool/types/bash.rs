//! Typed result for the `bash` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `bash` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub signal: Option<i32>,
}

/// Typed input for the `bash` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BashInput {
    /// The bash command to execute
    pub command: String,
    /// Timeout in milliseconds (default 120000)
    pub timeout: Option<u64>,
}
