//! Typed input and result types for the `memory` tool.

use serde::{Deserialize, Serialize};

/// Durable Memory layer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayerInput {
    Global,
    Project,
}

/// Durable Memory category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategoryInput {
    Fact,
    Decision,
    Preference,
    Pattern,
    Pitfall,
}

/// Session reminder priority.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderPriorityInput {
    Low,
    Normal,
    High,
}

/// Storage location of a Memory search hit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLocationResult {
    Active,
    Archive,
}

/// Structured persistent Memory entry returned to the caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEntryResult {
    pub id: String,
    pub content: String,
    pub layer: MemoryLayerInput,
    pub category: MemoryCategoryInput,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub outdated: bool,
    pub ttl_expired: bool,
    /// Set when another memory replaced this one (#1774). Such an entry is no
    /// longer injected but stays visible here so the model can see the history.
    pub superseded_by: Option<String>,
    /// Memory ids this entry was merged from (#1775). Historical merges carry
    /// no pointers — the evidence chain only starts with new merges.
    pub evidence: Vec<String>,
}

/// Structured explicit-search hit with deterministic relevance metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemorySearchHitResult {
    pub id: String,
    pub content: String,
    pub layer: MemoryLayerInput,
    pub category: MemoryCategoryInput,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub location: MemoryLocationResult,
    pub outdated: bool,
    pub ttl_expired: bool,
    pub superseded_by: Option<String>,
    /// Memory ids this entry was merged from (#1775).
    pub evidence: Vec<String>,
    pub relevance: Option<f64>,
}

/// Actionable Memory capacity candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEvictionCandidateResult {
    pub id: String,
    pub content: String,
    pub layer: MemoryLayerInput,
    pub category: MemoryCategoryInput,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub outdated: bool,
    pub ttl_expired: bool,
    pub confirmation_count: u32,
    pub last_confirmed_at: u64,
    pub eviction_score: i64,
    pub eviction_reason: String,
}

/// Typed result returned by the `memory` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MemoryResult {
    pub action: String,
    pub id: Option<String>,
    pub entries: Option<Vec<MemoryEntryResult>>,
    pub hits: Option<Vec<MemorySearchHitResult>>,
    pub eviction_candidates: Option<Vec<MemoryEvictionCandidateResult>>,
}

/// Memory write status transition applied by `MemoryUpdate`.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Pin,
    Unpin,
    Archive,
    Restore,
}

/// Input for the `MemoryAdd` tool: write one persistent memory entry.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryAddInput {
    /// 要记住的文本，上限 500 字符。Content to remember, up to 500 chars.
    pub content: String,
    /// 层级：global 仅用于明确跨项目适用的偏好。默认 project。
    /// Layer: global only for preferences explicitly applicable across projects. Defaults to project.
    pub layer: Option<MemoryLayerInput>,
    /// 分类：fact 事实 / decision 决策 / preference 偏好 / pattern 模式 / pitfall 陷阱。默认 fact。
    /// Category: fact / decision / preference / pattern / pitfall. Defaults to fact.
    pub category: Option<MemoryCategoryInput>,
    /// 标签，上限 10 个，每个不超过 32 字符。Tags, up to 10, each up to 32 chars.
    pub tags: Option<Vec<String>>,
    /// 是否置顶以避免容量淘汰。Pin to protect from capacity eviction.
    pub pinned: Option<bool>,
}

/// Input for the `MemorySearch` tool: lexical search over memory entries.
#[derive(Debug, Clone, Deserialize)]
pub struct MemorySearchInput {
    /// 检索词，使用少量辨识词。Search terms; use a few discriminating words.
    pub query: String,
    /// 返回条数，上限 50。默认 10。Max results, capped at 50. Defaults to 10.
    pub limit: Option<u64>,
    /// 限定层级。Restrict to a layer.
    pub layer: Option<MemoryLayerInput>,
    /// 限定分类。Restrict to a category.
    pub category: Option<MemoryCategoryInput>,
}

/// Input for the `MemoryList` tool: list memory entries.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryListInput {
    /// 限定层级。Restrict to a layer.
    pub layer: Option<MemoryLayerInput>,
}

/// Input for the `MemoryUpdate` tool: pin, unpin, archive, or restore one entry.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryUpdateInput {
    /// 目标记忆 ID。Target memory ID.
    pub id: String,
    /// 状态变更：pin 置顶 / unpin 取消置顶 / archive 归档 / restore 恢复。
    /// Status transition: pin / unpin / archive / restore.
    pub status: MemoryStatus,
}

/// Input for the `MemoryDelete` tool: permanently delete one entry.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryDeleteInput {
    /// 目标记忆 ID。Target memory ID.
    pub id: String,
}
