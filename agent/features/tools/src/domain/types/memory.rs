//! Typed input and result types for the `memory` tool.

use serde::{Deserialize, Serialize};

/// Supported Memory actions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAction {
    Add,
    Delete,
    Search,
    Pin,
    #[default]
    List,
    AddReminder,
    CompleteReminder,
}

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
    pub relevance: Option<f64>,
}

/// Typed result returned by the `memory` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MemoryResult {
    pub action: String,
    pub id: Option<String>,
    pub entries: Option<Vec<MemoryEntryResult>>,
    pub hits: Option<Vec<MemorySearchHitResult>>,
}

/// Typed input for the `memory` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemoryInput {
    /// Memory action to perform
    pub action: MemoryAction,
    /// Memory id for delete, pin, or complete_reminder actions
    pub id: Option<String>,
    /// Persistent memory or session reminder content, max 500 chars
    pub content: Option<String>,
    /// Lexical search query for persistent memory
    pub query: Option<String>,
    /// Maximum number of search or list results
    pub limit: Option<u64>,
    /// Persistent memory layer; global applies across projects, project applies only to the current project
    pub layer: Option<MemoryLayerInput>,
    /// Persistent memory category
    pub category: Option<MemoryCategoryInput>,
    /// Optional persistent memory tags
    pub tags: Option<Vec<String>>,
    /// Whether to pin persistent memory
    pub pinned: Option<bool>,
    /// Session reminder priority; reminders are not persistent memory
    pub priority: Option<ReminderPriorityInput>,
}
