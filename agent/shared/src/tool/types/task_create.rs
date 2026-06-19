//! Typed result for the `task_create` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_create` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskCreateResult {
    pub task_id: String,
}

/// Typed input for the `task_create` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// camelCase 属性名（`activeForm`、`sessionId`）通过 camelCase 字段名直接产出，
/// 与现有手写 schema 完全一致。
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct TaskCreateInput {
    /// A brief title for the task
    pub subject: String,
    /// What needs to be done
    pub description: String,
    /// Present continuous form for spinner display
    pub activeForm: Option<String>,
    /// Task priority level
    pub priority: Option<String>,
    /// Session ID to associate with this task
    pub sessionId: Option<String>,
    /// Task owner
    pub owner: Option<String>,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
}
