//! Typed result for the `task_get` tool (non-core tool).

use super::task::Task;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_get` tool.
///
/// Re-uses the canonical `share::tool::types::task::Task` type so task results stay
/// interoperable with the rest of the task subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: Task,
}
