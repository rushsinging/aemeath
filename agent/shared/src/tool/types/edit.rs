//! Typed result for the `edit` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `edit` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EditResult {
    pub file_path: String,
    pub replacements_made: u64,
    pub dry_run: bool,
}

/// Typed input for the `edit` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct EditInput {
    /// Absolute path to the file
    pub file_path: String,
    /// The exact text to replace
    pub old_string: String,
    /// The replacement text
    pub new_string: String,
    /// Replace all occurrences (default false)
    pub replace_all: Option<bool>,
}
