//! Typed result for the `task_get` tool (non-core tool).

use crate::task::types::Task;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `task_get` tool.
///
/// Re-uses the canonical `share::task::Task` type so task results stay
/// interoperable with the rest of the task subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, ToolSchema)]
pub struct TaskGetResult {
    pub task: Task,
}