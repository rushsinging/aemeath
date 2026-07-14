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
/// 采用 key-value 模式：每次只更新**一个**字段，从根本上消除 LLM
/// 给可选参数填占位符的问题（#979 根因修复）。
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskUpdateInput {
    /// The ID of the task to update
    #[serde(alias = "taskId")]
    pub task_id: String,
    /// Field to update. One of: status, subject, description, active_form, owner, priority, progress, progress_message, add_blocked_by, add_blocks, add_tags, remove_tags
    pub key: String,
    /// New value for the field. Type depends on key: string for most fields, integer (0-100) for progress, array of task-id strings for add_blocked_by/add_blocks, array of tag strings for add_tags/remove_tags
    pub value: serde_json::Value,
}
