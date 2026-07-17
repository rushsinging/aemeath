//! Typed result for the `task_stop` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `task_stop` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskStopResult {
    pub task_id: String,
}

/// Typed input for the `task_stop` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TaskStopInput {
    /// The ID of the task to stop
    #[serde(alias = "taskId")]
    pub task_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_input() {
        let json = serde_json::json!({"task_id": "42"});
        let input: TaskStopInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.task_id, "42");
    }

    #[test]
    fn legacy_camel_case_alias() {
        let json = serde_json::json!({"taskId": "42"});
        let input: TaskStopInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.task_id, "42");
    }
}
