//! Typed result for the `task_list` tool (non-core tool).

use crate::task::types::Task;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `task_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize, ToolSchema)]
pub struct TaskListResult {
    pub tasks: Vec<Task>,
}