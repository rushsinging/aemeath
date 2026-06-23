//! Typed result for the `task_update` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_update` tool.
///
/// `subject` 由工具从 store 回填（LLM 调用时通常不传 subject），供 TUI 渲染
/// header 使用（issue #486）：`Task N — <subject> → <status>`。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskUpdateResult {
    pub task_id: String,
    pub status: String,
    #[serde(default)]
    pub subject: String,
}

/// Typed input for the `task_update` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// camelCase 属性名（`taskId`、`activeForm`、`progressMessage`、`addBlockedBy`、
/// `addBlocks`、`addTags`、`removeTags`）通过 camelCase 字段名直接产出，
/// 与现有手写 schema 完全一致。
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(non_snake_case)]
pub struct TaskUpdateInput {
    /// The ID of the task to update
    pub taskId: String,
    pub status: Option<String>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub activeForm: Option<String>,
    pub owner: Option<String>,
    pub priority: Option<String>,
    /// Progress percentage (0-100)
    pub progress: Option<u8>,
    /// Progress status message
    pub progressMessage: Option<String>,
    pub addBlockedBy: Option<Vec<String>>,
    pub addBlocks: Option<Vec<String>>,
    pub addTags: Option<Vec<String>>,
    pub removeTags: Option<Vec<String>>,
}
