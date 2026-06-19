//! Typed result for the `enter_worktree` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;
use std::path::PathBuf;

/// Typed result returned by the `enter_worktree` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct EnterWorktreeResult {
    pub branch: String,
    pub path_base: PathBuf,
    pub working_root: PathBuf,
    pub guidance: String,
}