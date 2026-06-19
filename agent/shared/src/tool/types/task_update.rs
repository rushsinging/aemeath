//! Typed result for the `task_update` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_update` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {task_id: string, status: string}
pub struct TaskUpdateResult {
    pub task_id: String,
    pub status: String,
}