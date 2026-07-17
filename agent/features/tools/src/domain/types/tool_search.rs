//! Typed result for the `tool_search` tool (non-core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 单个工具的详细信息。
///
/// 由 `ToolSearch` 返回，供 LLM 了解工具的完整能力。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolInfo {
    /// 工具名称
    pub name: String,
    /// 工具描述
    pub description: String,
    /// 输入参数 JSON Schema
    pub input_schema: Value,
    /// 是否只读（不产生副作用）
    pub is_read_only: bool,
}

/// Typed result returned by the `tool_search` tool.
///
/// `tools` 包含所有匹配搜索条件的工具详细信息，按相关度排序。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolSearchResult {
    pub tools: Vec<ToolInfo>,
}

/// Typed input for the `tool_search` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToolSearchInput {
    /// Search query - tool name or functionality keyword
    pub query: String,
}
