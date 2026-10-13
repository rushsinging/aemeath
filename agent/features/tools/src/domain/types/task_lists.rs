use serde::{Deserialize, Serialize};

use super::task_list::{TaskListMetadata, TaskListStats};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskListsInput {
    /// Short intent for this call (3-5 words). Shown in TUI header / background summary.
    pub goal: String,
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
