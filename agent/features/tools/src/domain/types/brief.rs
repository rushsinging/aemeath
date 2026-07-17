//! Typed result for the `brief` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `brief` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BriefResult {
    pub summary: String,
}

/// Typed input for the `brief` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BriefInput {
    /// Output format for the brief
    pub format: Option<String>,
    /// What to include in the brief (default: all)
    pub include: Option<Vec<String>>,
    /// Custom title for the brief (optional)
    pub title: Option<String>,
}
