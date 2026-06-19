//! Typed result for the `sleep` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `sleep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SleepResult {
    pub duration_ms: u64,
}

/// Typed input for the `sleep` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SleepInput {
    /// Duration to sleep in milliseconds (max 60000)
    pub duration_ms: u64,
}
