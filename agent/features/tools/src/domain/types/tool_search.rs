//! Typed result for the `tool_search` tool (non-core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// ToolSearch 命中条目的来源类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchItemKind {
    /// 内置 / MCP 等可执行工具。
    #[default]
    Tool,
    /// Skill 元数据（需再经 Skill 工具加载正文）。
    Skill,
}

/// 单个工具或 Skill 的详细信息。
///
/// 由 `ToolSearch` 返回，供 LLM 了解能力；Skill 条目的 `input_schema` 为空对象。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolInfo {
    /// 名称（工具名或 skill identity）
    pub name: String,
    /// 描述（工具 description 或 skill frontmatter description）
    pub description: String,
    /// 输入参数 JSON Schema（Skill 为空对象）
    pub input_schema: Value,
    /// 是否只读（不产生副作用）；Skill 恒为 true
    pub is_read_only: bool,
    /// 条目类别
    #[serde(default)]
    pub kind: SearchItemKind,
}

/// Typed result returned by the `tool_search` tool.
///
/// `tools` 包含所有匹配搜索条件的工具/Skill 详细信息，按相关度排序。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolSearchResult {
    pub tools: Vec<ToolInfo>,
}

/// Typed input for the `tool_search` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ToolSearchInput {
    /// Short intent for this call (3-5 words). Shown in TUI header / background summary.
    pub goal: String,
    /// Search query - tool/skill name or functionality keyword
    pub query: String,
}
