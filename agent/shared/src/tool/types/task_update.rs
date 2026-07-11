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
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskUpdateInput {
    /// The ID of the task to update
    #[serde(alias = "taskId")]
    pub task_id: String,
    pub status: Option<String>,
    pub subject: Option<String>,
    pub description: Option<String>,
    #[serde(alias = "activeForm")]
    pub active_form: Option<String>,
    pub owner: Option<String>,
    pub priority: Option<String>,
    /// Progress percentage (0-100)
    pub progress: Option<u8>,
    /// Progress status message
    #[serde(alias = "progressMessage")]
    pub progress_message: Option<String>,
    #[serde(alias = "addBlockedBy")]
    pub add_blocked_by: Option<Vec<String>>,
    #[serde(alias = "addBlocks")]
    pub add_blocks: Option<Vec<String>>,
    #[serde(alias = "addTags")]
    pub add_tags: Option<Vec<String>>,
    #[serde(alias = "removeTags")]
    pub remove_tags: Option<Vec<String>>,
}
