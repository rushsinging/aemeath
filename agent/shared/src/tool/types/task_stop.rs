//! Typed result for the `task_stop` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_stop` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskStopResult {
    pub task_id: String,
}

/// Typed input for the `task_stop` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// camelCase 属性名（`taskId`）通过 camelCase 字段名直接产出，
/// 与现有手写 schema 完全一致。
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct TaskStopInput {
    /// The ID of the task to stop
    pub taskId: String,
}
