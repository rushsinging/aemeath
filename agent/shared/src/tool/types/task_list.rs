//! Typed result for the `task_list` tool (non-core tool).

use super::task::Task;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListResult {
    pub tasks: Vec<Task>,
}

/// Typed input for the `task_list` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// camelCase 属性名（`sessionId`）通过 camelCase 字段名直接产出，
/// 与现有手写 schema 完全一致。所有字段可选（原 schema 无 required）。
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct TaskListInput {
    /// Filter by status
    pub status: Option<String>,
    /// Filter by priority
    pub priority: Option<String>,
    /// Filter by session ID
    pub sessionId: Option<String>,
}
