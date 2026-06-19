//! Typed result for the `task_get` tool (non-core tool).

use super::task::Task;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_get` tool.
///
/// Re-uses the canonical `share::tool::types::task::Task` type so task results stay
/// interoperable with the rest of the task subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: Task,
}

/// Typed input for the `task_get` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// camelCase 属性名（`taskId`）通过 camelCase 字段名直接产出，
/// 与现有手写 schema 完全一致。
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct TaskGetInput {
    /// The ID of the task to retrieve
    pub taskId: String,
}
