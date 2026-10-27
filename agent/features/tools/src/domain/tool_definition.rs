//! 工具定义的发布类型（schema 真相源）。
//!
//! #1863 W2：`ToolDefinition` 归 tools（工具 schema 的单一类型定义）；
//! runtime 收集 wire → 类型化，context 估算/组装全程稳定类型，JSON
//! 序列化只发生在 provider 请求出口。

use serde::{Deserialize, Serialize};

/// 一个工具的完整定义（provider 请求与上下文预算共用的稳定类型）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl ToolDefinition {
    /// 从 wire JSON（`{name, description, input_schema}`）反构；缺字段返回 `None`。
    pub fn from_wire(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            name: value.get("name")?.as_str()?.to_string(),
            description: value.get("description")?.as_str()?.to_string(),
            input_schema: value.get("input_schema")?.clone(),
        })
    }
}
