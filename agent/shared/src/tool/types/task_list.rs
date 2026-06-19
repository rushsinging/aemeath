//! Typed result for the `task_list` tool (non-core tool).

use crate::task::types::Task;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskListResult {
    pub tasks: Vec<Task>,
}