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
/// 所有字段可选（原 schema 无 required）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskListInput {
    /// Filter by status
    pub status: Option<String>,
    /// Filter by priority
    pub priority: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::types::ToolSchema;

    #[test]
    fn task_list_schema_does_not_publish_session_id() {
        let schema = TaskListInput::data_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("task list schema properties");
        assert!(!properties.contains_key("session_id"));
        assert!(!properties.contains_key("sessionId"));
        assert!(properties.contains_key("status"));
        assert!(properties.contains_key("priority"));
    }
}
