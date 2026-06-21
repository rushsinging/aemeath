//! Typed result for the `bash` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `bash` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub signal: Option<i32>,
    /// 命令执行后 path_base 发生变化（如 `cd subdir`）时回填的新 path_base（#414）。
    /// 仅在变化时为 `Some`，未变时为 `None`，减少噪音。历史 tool result 反序列化时缺省为 `None`。
    #[serde(default)]
    pub path_base: Option<PathBuf>,
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
