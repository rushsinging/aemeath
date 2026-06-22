//! Session types shared across crates.
//!
//! These types are defined in core because they are referenced by project and
//! runtime crates. The full session implementation lives in runtime::session.

use serde::{Deserialize, Serialize};

/// Workspace context for worktree support — persisted session DTO.
///
/// `workspace_root` 经 #440 从 `working_root` 重命名，`#[serde(alias)]`
/// 保留对旧 session 文件（落盘 JSON）的向后兼容。
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceContext {
    pub path_base: String,
    #[serde(alias = "working_root")]
    pub workspace_root: String,
    #[serde(default)]
    pub context_stack: Vec<PersistedWorkspaceFrame>,
}

/// An entry in the persisted workspace context stack (for nested worktrees).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceFrame {
    pub path_base: String,
    #[serde(alias = "working_root")]
    pub workspace_root: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_workspace_context_serde_field_compat() {
        let json = r#"{"path_base":"/a","workspace_root":"/b","context_stack":[{"path_base":"/c","workspace_root":"/d"}]}"#;
        let ctx: PersistedWorkspaceContext = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.path_base, "/a");
        assert_eq!(ctx.workspace_root, "/b");
        assert_eq!(ctx.context_stack.len(), 1);
        assert_eq!(ctx.context_stack[0].path_base, "/c");
        let _legacy: PersistedWorkspaceContext = ctx.clone();
        let back = serde_json::to_string(&ctx).unwrap();
        assert_eq!(back, json);
    }

    /// 旧 session 文件用 `working_root` 字段名，经 alias 应能正确反序列化（#440 向后兼容）。
    #[test]
    fn persisted_workspace_context_accepts_legacy_working_root_alias() {
        let legacy = r#"{"path_base":"/a","working_root":"/b","context_stack":[{"path_base":"/c","working_root":"/d"}]}"#;
        let ctx: PersistedWorkspaceContext = serde_json::from_str(legacy).unwrap();
        assert_eq!(ctx.workspace_root, "/b");
        assert_eq!(ctx.context_stack[0].workspace_root, "/d");
        // 再序列化时应输出新的 workspace_root 字段名
        let back = serde_json::to_string(&ctx).unwrap();
        assert!(
            back.contains(r#""workspace_root":"#),
            "serialize 应输出新字段名: {back}"
        );
        assert!(
            !back.contains(r#""working_root":"#),
            "序列化不应再含旧字段名: {back}"
        );
    }
}
