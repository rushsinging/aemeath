//! Typed result for the `task_list_complete` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_list_complete` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskListCompleteResult {
    pub batch_id: String,
}
