//! Typed result for the `task_list` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListResult {
    pub tasks: Vec<task::TaskView>,
}

/// Typed input for the `task_list` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// 所有字段可选（原 schema 无 required）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskListInput {
    /// Filter by status
    pub status: Option<String>,
    /// Filter by priority
    pub priority: Option<String>,
}

#[cfg(test)]
#[path = "task_list_tests.rs"]
mod tests;
