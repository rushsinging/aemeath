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
///
/// `timeout` **MUST NOT** 在此声明：它是框架级通用参数，由 catalog 统一注入到
/// 发布给模型的 schema，并在执行前由框架摘除、解析成 effective 值写入
/// `ExecutionScope`。工具只读 `ctx.effective_timeout_secs()`。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct BashInput {
    /// A short description of the command goal/intent, shown as the TUI header.
    pub goal: String,
    /// The bash command to execute
    pub command: String,
}
