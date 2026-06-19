//! Typed result for the `task_list_create` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_list_create` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {batch_id: string}
pub struct TaskListCreateResult {
    pub batch_id: String,
}
