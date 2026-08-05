use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TaskStateView {
    pub session_id: String,
    pub revision: u64,
    pub current_batch: Option<TaskBatchView>,
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub items: Vec<TaskItemView>,
    pub hidden_count: usize,
}

impl TaskStateView {
    pub fn empty(session_id: impl Into<String>, revision: u64) -> Self {
        Self {
            session_id: session_id.into(),
            revision,
            current_batch: None,
            total: 0,
            completed: 0,
            in_progress: 0,
            items: Vec::new(),
            hidden_count: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TaskBatchView {
    pub id: u64,
    pub summary: Option<String>,
    pub status: TaskBatchStatusView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskBatchStatusView {
    Active,
    Paused,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TaskItemView {
    pub id: u64,
    pub sequence: u64,
    pub subject: String,
    pub status: TaskItemStatusView,
    pub priority: TaskPriorityView,
    pub blocked_by_sequences: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskItemStatusView {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriorityView {
    Low,
    Normal,
    High,
    Urgent,
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
