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
#[derive(Debug, Clone, Deserialize, Default)]
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
