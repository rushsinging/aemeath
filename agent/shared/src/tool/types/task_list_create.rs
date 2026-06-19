//! Typed result for the `task_list_create` tool (non-core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `task_list_create` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct TaskListCreateResult {
    pub task_id: String,
}