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
    /// 当前进度百分比 (0-100)
    #[serde(default)]
    pub progress: u8,
    /// 当前被阻塞的任务 id 列表
    #[serde(default)]
    pub blocked_by: Vec<String>,
}

/// Typed input for the `task_update` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskUpdateInput {
    /// The ID of the task to update
    #[serde(alias = "taskId")]
    pub task_id: String,
    /// New status. Omit if not changing — do NOT pass placeholder values.
    pub status: Option<String>,
    /// New subject. Omit if not changing — do NOT pass placeholder values.
    pub subject: Option<String>,
    /// New description. Omit if not changing — do NOT pass placeholder values.
    pub description: Option<String>,
    /// New present continuous form for spinner. Omit if not changing — do NOT pass placeholder values.
    #[serde(alias = "activeForm")]
    pub active_form: Option<String>,
    /// New owner. Omit if not changing — do NOT pass placeholder values.
    pub owner: Option<String>,
    /// New priority level. Omit if not changing — do NOT pass placeholder values.
    pub priority: Option<String>,
    /// Progress percentage (0-100). Omit if not changing.
    pub progress: Option<u8>,
    /// Progress status message. Omit if not changing — do NOT pass placeholder values.
    #[serde(alias = "progressMessage")]
    pub progress_message: Option<String>,
    /// Task IDs that block this task
    #[serde(alias = "addBlockedBy")]
    pub add_blocked_by: Option<Vec<String>>,
    /// Task IDs that this task blocks
    #[serde(alias = "addBlocks")]
    pub add_blocks: Option<Vec<String>>,
    /// Tags to add
    #[serde(alias = "addTags")]
    pub add_tags: Option<Vec<String>>,
    /// Tags to remove
    #[serde(alias = "removeTags")]
    pub remove_tags: Option<Vec<String>>,
}
