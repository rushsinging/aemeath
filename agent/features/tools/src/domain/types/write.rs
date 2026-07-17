//! Typed result for the `write` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `write` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WriteResult {
    pub file_path: String,
    pub bytes_written: u64,
}

/// Typed input for the `write` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct WriteInput {
    /// Absolute or workspace-relative path of the file to create or overwrite. Required.
    pub file_path: String,
    /// Full text content to write into the file. Required (use empty string for an empty file).
    pub content: String,
}
