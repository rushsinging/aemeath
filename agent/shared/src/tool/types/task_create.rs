//! Typed result for the `task_create` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_create` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskCreateResult {
    pub task_id: String,
}
