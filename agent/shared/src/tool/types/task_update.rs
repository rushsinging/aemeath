//! Typed result for the `task_update` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_update` tool.
///
/// 回填完整 task 状态，供 LLM 获得上下文、TUI 渲染 header 使用（#979）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskUpdateResult {
    pub task_id: String,
    pub status: String,
    #[serde(default)]
    pub subject: String,
    /// 当前优先级（如 "high"）
    #[serde(default)]
    pub priority: String,
    /// 当前被阻塞的任务 id 列表
    #[serde(default)]
    pub blocked_by: Vec<String>,
}

/// Typed input for the `task_update` tool.
///
/// 采用 key-value 模式：每次只更新**一个**字段，从根本上消除 LLM
/// 给可选参数填占位符的问题（#979 根因修复）。
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskUpdateInput {
    /// The ID of the task to update
    #[serde(alias = "taskId")]
    pub task_id: String,
    /// Field to update. One of: status, subject, description, owner, priority, blocked_by_id
    pub key: String,
    /// New value for the field (always a string). For blocked_by_id, pass a single task ID.
    pub value: serde_json::Value,
}
