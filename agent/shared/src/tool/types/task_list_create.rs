//! Typed result for the `task_list_create` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_list_create` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskListCreateResult {
    pub batch_id: String,
}

/// Typed input for the `task_list_create` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskListCreateInput {
    /// Short title for this task list
    pub subject: String,
    /// One-sentence summary of the user request this task list belongs to
    pub summary: String,
}
