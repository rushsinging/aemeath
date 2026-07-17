//! Typed result for the `edit` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `edit` tool.
///
/// `old`/`new`/`start_line` 携带 diff 内容供 TUI 渲染（issue #546），
/// 避免把 diff 塞进 `text`（给 LLM）造成 token 浪费——LLM 自己在 tool_use input
/// 里已写过 old_string/new_string，无需在 result 里再看一遍。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EditResult {
    pub file_path: String,
    pub replacements_made: u64,
    pub dry_run: bool,
    /// 被替换的原文（fuzzy 匹配后实际命中的内容）
    pub old: String,
    /// 替换后的新文（adapt_indentation 后实际写入的内容）
    pub new: String,
    /// 匹配起始行号（1-based）
    pub start_line: u64,
}

/// Typed input for the `edit` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
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
