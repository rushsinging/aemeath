#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectApiMarker;

// Workspace 领域类型与能力端口定义在 business（最内层所有者）；contract 仅向外 re-export。
pub use crate::business::workspace_types::{
    WorkspaceControl, WorkspaceError, WorkspaceFrame, WorkspacePersist, WorkspaceRead,
};
