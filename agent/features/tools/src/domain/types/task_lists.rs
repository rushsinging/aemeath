use serde::{Deserialize, Serialize};

use super::task_list::{TaskListMetadata, TaskListStats};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskListsInput {
    /// Filter task lists by status: active, paused, or archived
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListSummary {
    pub task_list: TaskListMetadata,
    pub stats: TaskListStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListsResult {
    pub task_lists: Vec<TaskListSummary>,
}
