//! Typed result for the `task_list` tool (non-core tool).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListMetadata {
    pub id: String,
    pub summary: String,
    pub status: String,
    pub created_at: u64,
    pub last_active_turn: u64,
    pub silence_turns: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TaskListStats {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
}

/// Typed result returned by the `task_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListResult {
    pub task_list: Option<TaskListMetadata>,
    pub stats: TaskListStats,
    pub tasks: Vec<task::TaskView>,
}

/// Typed input for the `task_list` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// 所有字段可选（原 schema 无 required）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskListInput {
    /// Task list ID to query; defaults to the current active list
    pub task_list_id: Option<String>,
    /// Filter by status
    pub status: Option<String>,
    /// Filter by priority
    pub priority: Option<String>,
}

#[cfg(test)]
#[path = "task_list_tests.rs"]
mod tests;
