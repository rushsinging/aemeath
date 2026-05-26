//! worktree 操作的公共接口
//!
//! tools 通过此模块调用 project 的 worktree 函数，
//! 避免直接依赖 project crate（门禁不允许 tools→project）。

pub use project::worktree::{
    enter_worktree, exit_worktree, get_git_common_dir, is_same_git_repo, restore_workspace_context,
    workspace_context_from_tool_context,
};
