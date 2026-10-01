//! Session types shared across crates.
//!
//! These types are defined in core because they are referenced by project and
//! runtime crates. The full session implementation lives in runtime::session.

use serde::{Deserialize, Serialize};

/// Project-owned identity published to Session and other bounded contexts.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectIdentityData {
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
    /// 直接构造（持久化反序列化与测试；生产派生走 `derive`）。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derive a deterministic opaque identifier without exposing path semantics.
    pub fn derive(identity: &ProjectIdentityData, workspace_root: &str) -> Self {
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

fn identity_is_default(value: &ProjectIdentityData) -> bool {
    value == &ProjectIdentityData::default()
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
    pub project_identity: ProjectIdentityData,
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
#[path = "session_types_tests.rs"]
mod tests;
