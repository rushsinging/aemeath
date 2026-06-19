//! Typed result for the `task_stop` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_stop` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskStopResult {
    pub task_id: String,
}