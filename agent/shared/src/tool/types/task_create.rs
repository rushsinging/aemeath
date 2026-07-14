//! Typed result for the `task_create` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_create` tool.
///
/// 回填完整 task 状态，供 LLM 获得上下文、TUI 渲染使用（#979）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskCreateResult {
    pub task_id: String,
    /// 显示编号（如 "1"），与 `task_id`（全局 uuid）区分
    #[serde(default)]
    pub display_id: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: String,
}

/// Typed input for the `task_create` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskCreateInput {
    /// A brief title for the task
    pub subject: String,
    /// What needs to be done
    pub description: String,
    /// Present continuous form for spinner display
    #[serde(alias = "activeForm")]
    pub active_form: Option<String>,
    /// Task priority level
    pub priority: Option<String>,
    /// Session ID to associate with this task
    #[serde(alias = "sessionId")]
    pub session_id: Option<String>,
    /// Task owner
    pub owner: Option<String>,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
}
