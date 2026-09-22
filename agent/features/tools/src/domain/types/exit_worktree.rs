//! Typed result for the `exit_worktree` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `exit_worktree` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExitWorktreeResult {
    pub branch: String,
    pub path_base: PathBuf,
    #[serde(alias = "working_root")]
    pub workspace_root: PathBuf,
    /// 退出/切换后的面向 LLM guidance（#415：与 EnterWorktree 对称）。
    /// 明确 path_base（相对路径解析基）与 workspace_root（安全边界）语义（#413）。
    #[serde(default)]
    pub guidance: String,
}

/// Typed input for the `exit_worktree` tool.
///
/// build.rs 由本 struct 生成 `input_schema`。ExitWorktree 无输入字段且
/// 拒绝未知字段（含已移除的 `path`），防止误用直接切换旁路。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExitWorktreeInput {}
