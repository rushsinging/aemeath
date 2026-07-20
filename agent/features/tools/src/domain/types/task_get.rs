//! Typed result for the `task_get` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_get` tool.
///
/// Uses the Task-owned stable output view so Tool wire compatibility does not
/// depend on a duplicate Shared Kernel DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: task::TaskView,
}

#[cfg(test)]
#[path = "task_get_tests.rs"]
mod tests;

/// Typed input for the `task_get` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskGetInput {
    /// The ID of the task to retrieve
    #[serde(alias = "taskId")]
    pub task_id: String,
}
