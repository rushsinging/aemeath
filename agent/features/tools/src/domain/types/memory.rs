//! Typed result for the `memory` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `memory` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MemoryResult {
    pub action: String,
}

/// Typed input for the `memory` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
/// 枚举型参数（action/layer/category/priority）暂用 `String`。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemoryInput {
    /// Memory action to perform
    pub action: String,
    /// Memory id for delete/pin actions
    pub id: Option<String>,
    /// Memory content, max 500 chars
    pub content: Option<String>,
    /// Search query
    pub query: Option<String>,
    /// Maximum number of results
    pub limit: Option<u64>,
    /// Memory layer
    pub layer: Option<String>,
    /// Memory category
    pub category: Option<String>,
    /// Optional tags
    pub tags: Option<Vec<String>>,
    /// Whether to pin the memory
    pub pinned: Option<bool>,
    /// Reminder priority
    pub priority: Option<String>,
}
