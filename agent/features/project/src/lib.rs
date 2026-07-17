/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:project";

mod adapters;
mod domain;

pub use adapters::git::GitCli;
pub use domain::git::GitWorktreeOps;
pub use domain::service::WorkspaceService;
pub use domain::types::{
    WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspacePersist, WorkspaceRead,
};
