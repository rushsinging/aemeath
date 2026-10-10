//! Typed result for the `grep` tool (issue #273 core tool).

use super::support::Match;
use serde::{Deserialize, Serialize};

/// Grep 输出模式。
///
/// 默认 `files_with_matches`：只返回「文件 + 匹配数」索引。Grep 的价值是发现性
/// 信息（哪里命中），索引体积受字符预算控制，不会因落盘预览而丢失位置清单。
/// `content` 保留逐行匹配（`path:line: text`）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutputMode {
    #[default]
    FilesWithMatches,
    Content,
}

/// 索引模式下的单个命中文件。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GrepFileMatch {
    /// 相对 workspace root 的路径（无法相对化时保留检索命令返回的原路径）。
    pub file_path: String,
    /// 该文件的匹配数。
    pub match_count: u64,
}

/// Typed result returned by the `grep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GrepResult {
    pub matches: Vec<Match>,
    /// 真实匹配总行数（可能大于 `matches.len()`）。
    pub total_matches: u64,
    /// 实际返回的条目数：content 模式为匹配行数（`matches.len()`），
    /// 索引模式为文件数（`files.len()`），便于消费方判断是否被截断。
    pub shown: u64,
    pub query: String,
    /// 索引模式：命中的文件与匹配数（按匹配数降序，受字符预算截断）。
    pub files: Vec<GrepFileMatch>,
    /// 索引模式：命中文件总数（可能大于 `files.len()`）。
    pub total_files: u64,
}

/// Typed input for the `grep` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GrepInput {
    /// Regex pattern to search for
    pub pattern: String,
    /// File or directory to search in (defaults to cwd)
    pub path: Option<String>,
    /// File glob filter (e.g. "*.rs")
    pub glob: Option<String>,
    /// Content mode: maximum number of matching lines to return (default: all).
    /// Files-with-matches mode: maximum number of files to list (default: budget-limited).
    pub head_limit: Option<u32>,
    /// Output mode. `files_with_matches` (default) returns a file index with match counts,
    /// which keeps the "where are the matches" map compact; `content` returns matching lines.
    pub output_mode: GrepOutputMode,
}
