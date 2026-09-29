//! Application 层：workspace 用例编排。
//!
//! 分工（Hexagonal 内层语义）：
//! - `domain::state::WorkspaceState`：聚合根，负责不变量（containment / git 位置 /
//!   栈形态）与状态迁移（enter / exit / change_directory / restore）；
//! - **本层**：用例编排——把「live 状态 + 出站 port」编排成「新状态」，纯函数、
//!   不持锁、不直接做 IO 决策（IO 在 `GitWorktreeOps` 实现里）；
//! - `domain::service::WorkspaceService`（infrastructure）：持锁、调用本层、写回，
//!   并实现三个对外 port trait。
//!
//! 用例失败绝不写回 live 状态（先在候选状态上验证成功才提交）。

use std::path::PathBuf;

use share::error::DomainError;
use share::session_types::PersistedWorkspaceContext;

use crate::domain::git::GitWorktreeOps;
use crate::domain::state::{WorkspaceRestoreData, WorkspaceState};
use crate::domain::types::WorkspaceData;

/// 变更当前工作目录：候选状态上校验 containment，成功后返回新状态。
pub fn change_directory(
    live: &WorkspaceState,
    path: PathBuf,
) -> Result<WorkspaceState, DomainError> {
    let mut candidate = live.clone();
    candidate.change_directory(path)?;
    Ok(candidate)
}

/// 进入（必要时创建）linked worktree：返回新状态与被压栈的前一帧。
pub fn enter_worktree(
    live: &WorkspaceState,
    git: &dyn GitWorktreeOps,
    path: Option<PathBuf>,
    branch: Option<String>,
    base: Option<String>,
) -> Result<(WorkspaceState, WorkspaceData), DomainError> {
    let mut candidate = live.clone();
    let frame = candidate.enter(git, path, branch, base)?;
    Ok((candidate, frame))
}

/// 退出当前 worktree：返回新状态与弹出的帧。
pub fn exit_worktree(
    live: &WorkspaceState,
    git: &dyn GitWorktreeOps,
) -> Result<(WorkspaceState, WorkspaceData), DomainError> {
    let mut candidate = live.clone();
    let frame = candidate.exit(git)?;
    Ok((candidate, frame))
}

/// 校验持久化快照并构造恢复令牌；只读 live 状态，失败不产生任何副作用。
pub fn prepare_workspace_restore(
    live: &WorkspaceState,
    dto: &PersistedWorkspaceContext,
    git: &dyn GitWorktreeOps,
) -> Result<WorkspaceRestoreData, DomainError> {
    WorkspaceState::prepare_restore(live, dto, git).map_err(Into::into)
}
