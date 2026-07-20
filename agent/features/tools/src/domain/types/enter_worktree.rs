//! Typed result for the `enter_worktree` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `enter_worktree` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EnterWorktreeResult {
    pub branch: String,
    pub path_base: PathBuf,
    #[serde(alias = "working_root")]
    pub workspace_root: PathBuf,
    pub guidance: String,
}

/// Typed input for the `enter_worktree` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EnterWorktreeInput {
    /// 可选：worktree 根目录路径（绝对或相对路径）。无 path 时必须省略该字段，禁止传空字符串；系统从 branch 推导为 .worktrees/<安全分支名>
    pub path: Option<String>,
    /// 可选：目标路径不存在时创建的新分支名；path 省略时必须提供
    pub branch: Option<String>,
    /// 可选：创建新 worktree 的起点；省略、空串或纯空白时默认 main；进入已有 worktree 时忽略
    pub base: Option<String>,
}
