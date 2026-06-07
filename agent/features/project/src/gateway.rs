//! project crate 的 gateway 门面：发布 workspace 服务与 git 出站端口/适配器。
//!
//! 与其他 feature 一致，业务实现（`WorkspaceService` / `GitWorktreeOps` / `GitCli`）
//! 经 gateway re-export 进入 `api`，使 `api.rs` 只引用 `crate::contract` / `crate::gateway`。

pub use crate::business::git_ops::{GitCli, GitWorktreeOps};
pub use crate::business::workspace_service::WorkspaceService;
