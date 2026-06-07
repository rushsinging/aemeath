//! Session types shared across crates.
//!
//! These types are defined in core because they are referenced by project and
//! runtime crates. The full session implementation lives in runtime::session.

use serde::{Deserialize, Serialize};

/// Workspace context for worktree support — persisted session DTO.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceContext {
    pub path_base: String,
    pub working_root: String,
    #[serde(default)]
    pub context_stack: Vec<PersistedWorkspaceFrame>,
}

/// An entry in the persisted workspace context stack (for nested worktrees).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceFrame {
    pub path_base: String,
    pub working_root: String,
}

/// 迁移期兼容别名（后续阶段删除）。
pub type WorkspaceContext = PersistedWorkspaceContext;
pub type WorkspaceStackEntry = PersistedWorkspaceFrame;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_workspace_context_serde_field_compat() {
        let json = r#"{"path_base":"/a","working_root":"/b","context_stack":[{"path_base":"/c","working_root":"/d"}]}"#;
        let ctx: PersistedWorkspaceContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.path_base, "/a");
        assert_eq!(ctx.working_root, "/b");
        assert_eq!(ctx.context_stack.len(), 1);
        assert_eq!(ctx.context_stack[0].path_base, "/c");
        let _legacy: WorkspaceContext = ctx.clone();
        let back = serde_json::to_string(&ctx).unwrap();
        assert_eq!(back, json);
    }
}
