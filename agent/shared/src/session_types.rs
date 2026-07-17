//! Session types shared across crates.
//!
//! These types are defined in core because they are referenced by project and
//! runtime crates. The full session implementation lives in runtime::session.

use serde::{Deserialize, Serialize};

/// Project-owned identity published to Session and other bounded contexts.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectIdentity {
    /// Canonical cwd used when the project was initialized.
    pub initial_cwd: String,
    /// Canonical git common directory, or `None` for a valid non-git project.
    pub git_common_dir: Option<String>,
}

/// Stable, opaque identifier for a workspace root within a project identity.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derive a deterministic opaque identifier without exposing path semantics.
    pub fn derive(identity: &ProjectIdentity, workspace_root: &str) -> Self {
        // Versioned domain separation plus length-prefixing makes the wire derivation
        // unambiguous and leaves room for a future algorithm/schema migration.
        let digest = utils::stable_sha256_hex(
            b"aemeath.workspace-id.v1\0",
            &[
                identity.initial_cwd.as_bytes(),
                identity.git_common_dir.as_deref().unwrap_or("").as_bytes(),
                workspace_root.as_bytes(),
            ],
        );
        Self(format!("ws-{digest}"))
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for WorkspaceId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for WorkspaceId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Verified relationship between a workspace root and its repository.
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorktreeKind {
    #[default]
    NonGit,
    Primary,
    Linked,
}

fn identity_is_default(value: &ProjectIdentity) -> bool {
    value == &ProjectIdentity::default()
}

/// Workspace context for worktree support — persisted session DTO.
///
/// `workspace_root` 经 #440 从 `working_root` 重命名，`#[serde(alias)]`
/// 保留对旧 session 文件（落盘 JSON）的向后兼容。
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceContext {
    /// Legacy snapshots omit the new identity fields; the compatibility ACL upgrades them.
    #[serde(default, skip_serializing_if = "WorkspaceId::is_empty")]
    pub workspace_id: WorkspaceId,
    #[serde(default, skip_serializing_if = "identity_is_default")]
    pub project_identity: ProjectIdentity,
    pub path_base: String,
    #[serde(alias = "working_root")]
    pub workspace_root: String,
    #[serde(default)]
    pub worktree_kind: WorktreeKind,
    #[serde(default)]
    pub context_stack: Vec<PersistedWorkspaceFrame>,
}

/// An entry in the persisted workspace context stack (for nested worktrees).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct PersistedWorkspaceFrame {
    pub path_base: String,
    #[serde(alias = "working_root")]
    pub workspace_root: String,
    #[serde(default)]
    pub worktree_kind: WorktreeKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_sha256_derivation_is_fixed_and_stable() {
        let identity = ProjectIdentity {
            initial_cwd: "/repo".into(),
            git_common_dir: Some("/repo/.git".into()),
        };
        let expected = "ws-ffe63e8fd16e52df0f847f2dd7ac863e1241533145d2dbd2b9c8308e8a728a0a";
        assert_eq!(WorkspaceId::derive(&identity, "/repo").as_str(), expected);
        assert_eq!(WorkspaceId::derive(&identity, "/repo").as_str(), expected);
        assert_ne!(
            WorkspaceId::derive(&identity, "/repo/wt").as_str(),
            expected
        );
    }

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
        assert!(back.contains(r#""path_base":"/a""#), "{back}");
        assert!(back.contains(r#""workspace_root":"/b""#), "{back}");
        assert!(back.contains(r#""worktree_kind":"NonGit""#), "{back}");
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

    // ---- #894: 完整 DTO 内嵌 ProjectIdentity / WorkspaceId / WorktreeKind ----

    /// #894: `PersistedWorkspaceContext` 必须内嵌 project identity / workspace id /
    /// worktree kind 的 wire copy，且新字段全部参与 serde round-trip。
    #[test]
    fn persisted_workspace_context_embeds_identity_id_and_kind() {
        let ctx = PersistedWorkspaceContext {
            workspace_id: WorkspaceId::from("ws-repo-primary"),
            project_identity: ProjectIdentity {
                initial_cwd: "/repo".to_string(),
                git_common_dir: Some("/repo/.git".to_string()),
            },
            path_base: "/repo/sub".to_string(),
            workspace_root: "/repo".to_string(),
            worktree_kind: WorktreeKind::Primary,
            context_stack: vec![PersistedWorkspaceFrame {
                path_base: "/repo".to_string(),
                workspace_root: "/repo".to_string(),
                worktree_kind: WorktreeKind::Primary,
            }],
        };

        assert_eq!(ctx.workspace_id.as_str(), "ws-repo-primary");
        assert_eq!(ctx.project_identity.initial_cwd, "/repo");
        assert_eq!(
            ctx.project_identity.git_common_dir.as_deref(),
            Some("/repo/.git")
        );
        assert_eq!(ctx.worktree_kind, WorktreeKind::Primary);
        assert_eq!(ctx.context_stack[0].worktree_kind, WorktreeKind::Primary);

        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("workspace_id"), "{json}");
        assert!(json.contains("project_identity"), "{json}");
        assert!(json.contains("worktree_kind"), "{json}");
        let back: PersistedWorkspaceContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ctx);
    }

    /// #894: NonGit identity — `git_common_dir` 为 `None`、kind 为 `NonGit`、栈为空。
    #[test]
    fn persisted_workspace_context_supports_non_git_identity() {
        let ctx = PersistedWorkspaceContext {
            workspace_id: WorkspaceId::from("ws-plain-dir"),
            project_identity: ProjectIdentity {
                initial_cwd: "/tmp/plain".to_string(),
                git_common_dir: None,
            },
            path_base: "/tmp/plain".to_string(),
            workspace_root: "/tmp/plain".to_string(),
            worktree_kind: WorktreeKind::NonGit,
            context_stack: vec![],
        };

        assert!(ctx.project_identity.git_common_dir.is_none());
        assert_eq!(ctx.worktree_kind, WorktreeKind::NonGit);
        assert!(ctx.context_stack.is_empty());
    }
}
