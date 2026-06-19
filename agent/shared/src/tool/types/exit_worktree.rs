//! Typed result for the `exit_worktree` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `exit_worktree` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExitWorktreeResult {
    pub branch: String,
    pub path_base: PathBuf,
    pub working_root: PathBuf,
}
