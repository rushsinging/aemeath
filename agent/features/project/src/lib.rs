//! Project：workspace/worktree 的探测、装配与生命周期。
//!
//! # Published Language（四类语法 + DomainError）
//!
//! | 类 | 实体 | 说明 |
//! |---|---|---|
//! | 工厂 | `wire_production_workspace` | 返回 `Workspace`（域句柄） |
//! | Port | `run_git_command` + `GitCommandOutcome`（全仓唯一 git 子进程 spawn 窄面；`GitOperationError` 为其错误面） | context/runtime 统一经此执行 git |
//! | Role | `Workspace`（三窄面 accessor + 隔离派生；原 Wiring/Views 合并）、`WorkspaceReader`/`WorkspaceControl`/`WorkspaceWriter`（窄 trait） | |
//! | Data | `WorkspaceData`（快照：id 内嵌 + 路径 + kind；原 Frame/Id/Kind 三导出合并——Id/Kind 物理在 share::session_types，本 crate 不再转发） | |
//! | Error | `share::error::DomainError`（三错误统一折叠；细变体 crate 内） | 错误统一随 #1711 并入 |
//!
//! `ProjectIdentityData`/`WorkspaceId`/`WorktreeKind` 定义于 `share::session_types`，
//! 消费方直连 share（本 crate 零转发）。

mod adapters;
mod constants;
pub(crate) use constants::LOG_TARGET;
pub(crate) mod application;
mod domain;
pub(crate) mod service;

pub use adapters::git::{run_git_command, GitCommandOutcome};
pub use adapters::wiring::{wire_production_workspace, Workspace};
pub use domain::state::WorkspaceRestoreData;
pub use domain::types::GitOperationError;
pub use domain::types::{WorkspaceControl, WorkspaceData, WorkspaceReader, WorkspaceWriter};

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
