//! Session types shared across crates.
//!
//! These types are defined in core because they are referenced by project and
//! runtime crates. The full session implementation lives in runtime::session.

use serde::{Deserialize, Serialize};

/// Workspace context for worktree support.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceContext {
    pub path_base: String,
    pub working_root: String,
    #[serde(default)]
    pub context_stack: Vec<WorkspaceStackEntry>,
}

/// An entry in the workspace context stack (for nested worktrees).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceStackEntry {
    pub path_base: String,
    pub working_root: String,
}
