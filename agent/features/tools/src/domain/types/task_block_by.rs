//! Typed input and result for the `TaskBlockBy` tool.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskBlockByResult {
    pub task_id: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub blocked_by_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskBlockByInput {
    /// The ID of the task whose dependencies will be replaced
    #[serde(alias = "taskId")]
    pub id: String,
    /// The complete list of task IDs that must finish before this task; pass an empty list to clear dependencies
    #[serde(alias = "blockByIds")]
    pub block_by_ids: Vec<String>,
}

#[cfg(test)]
#[path = "task_block_by_tests.rs"]
mod tests;
