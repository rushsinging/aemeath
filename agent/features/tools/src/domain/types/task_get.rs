//! Typed result for the `task_get` tool (non-core tool).

use serde::{Deserialize, Serialize};
use share::task::types::Task;

/// Typed result returned by the `task_get` tool.
///
/// Re-uses the canonical `crate::domain::types::task::Task` type so task results stay
/// interoperable with the rest of the task subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: Task,
}

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
