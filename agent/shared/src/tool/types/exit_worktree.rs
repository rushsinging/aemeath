//! Typed result for the `exit_worktree` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `exit_worktree` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExitWorktreeResult {
    pub branch: String,
    pub path_base: PathBuf,
    pub working_root: PathBuf,
    /// 退出/切换后的面向 LLM guidance（#415：与 EnterWorktree 对称）。
    /// 明确 path_base（相对路径解析基）与 working_root（安全边界）语义（#413）。
    #[serde(default)]
    pub guidance: String,
}

/// Typed input for the `exit_worktree` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExitWorktreeInput {
    /// 可选：直接切回指定路径，忽略上下文栈
    pub path: Option<String>,
}
