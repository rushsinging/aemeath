//! Typed result for the `skill` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `skill` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SkillResult {
    pub name: String,
    pub path: String,
}

/// Typed input for the `skill` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct SkillInput {
    /// The skill name to execute
    pub skill: String,
    /// Optional arguments for the skill
    pub args: Option<String>,
}
