use serde::{Deserialize, Serialize};
use std::{fmt, time::Duration};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryId(uuid::Uuid);

impl MemoryId {
    pub fn new(value: impl AsRef<str>) -> Result<Self, MemoryError> {
        uuid::Uuid::parse_str(value.as_ref())
            .map(Self)
            .map_err(|_| MemoryError::InvalidEntry {
                message: "记忆 ID 必须是 UUID".to_string(),
            })
    }

    pub fn now_v7() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    pub fn as_uuid(&self) -> &uuid::Uuid {
        &self.0
    }
}

impl fmt::Display for MemoryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Fact,
    Decision,
    Preference,
    Pattern,
    Pitfall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    Llm,
    Hook,
    User,
}

/// 记忆来源类型，与 [`MemoryCategory`] 正交：分类表达用途，类型表达来源。
/// 合并产物与归纳产物都会持有 `evidence`，但读侧行为相反（归纳来源仍在
/// active 需要让位），因此必须显式标记，**NEVER** 用 `evidence.len()` 推导。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// 原始记忆（默认；合并产物亦属此类——其来源已归档，不参与注入）。
    #[default]
    Raw,
    /// 由多条记忆归纳而来的结论（其来源仍可能在 active，读侧需让位）。
    Synthesized,
}

impl MemoryKind {
    fn is_raw(kind: &MemoryKind) -> bool {
        matches!(kind, MemoryKind::Raw)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: MemoryId,
    pub layer: MemoryLayer,
    pub category: MemoryCategory,
    pub content: String,
    pub source: MemorySource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<Duration>,
    pub created_at: u64,
    #[serde(alias = "accessed_at")]
    pub last_confirmed_at: u64,
    #[serde(default, alias = "access_count")]
    pub confirmation_count: u32,
    #[serde(default)]
    pub outdated: bool,
    /// 被哪条记忆取代；`None` = 未被取代。只记录单向：反向关系可沿
    /// 全量条目扫描推导（#1774）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<MemoryId>,
    /// 记忆来源类型（#1775）。
    #[serde(default, skip_serializing_if = "MemoryKind::is_raw")]
    pub kind: MemoryKind,
    /// 来源条目：本条目由这些条目合并（写入去重，#1775）或归纳（#1776）
    /// 而来。可能指向同层 archive 条目，也可能指向 active 条目。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<MemoryId>,
}

impl MemoryEntry {
    pub fn new(
        id: MemoryId,
        now: u64,
        layer: MemoryLayer,
        category: MemoryCategory,
        content: impl Into<String>,
        source: MemorySource,
    ) -> Result<Self, MemoryError> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(MemoryError::InvalidEntry {
                message: "记忆内容不能为空".to_string(),
            });
        }
        Ok(Self {
            id,
            layer,
            category,
            content,
            source,
            source_ref: None,
            tags: Vec::new(),
            pinned: false,
            ttl: None,
            created_at: now,
            last_confirmed_at: now,
            confirmation_count: 0,
            outdated: false,
            superseded_by: None,
            kind: MemoryKind::default(),
            evidence: Vec::new(),
        })
    }

    pub fn is_ttl_expired(&self, now: u64) -> bool {
        self.ttl
            .map(|ttl| now > self.created_at.saturating_add(ttl.as_secs()))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MemoryError {
    #[error("记忆输入无效: {message}")]
    InvalidEntry { message: String },
    #[error("记忆不存在: {id}")]
    NotFound { id: MemoryId },
    #[error("记忆持久化失败: {kind}")]
    Storage { kind: MemoryStorageErrorKind },
    #[error("Reflection 部分应用（已完成 {result_completed}/{result_attempted}）")]
    PartialApply {
        result_attempted: usize,
        result_completed: usize,
        suggestions_added: usize,
        outdated_marked: usize,
        superseded: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MemoryStorageErrorKind {
    #[error("权限不足")]
    PermissionDenied,
    #[error("磁盘空间不足")]
    DiskFull,
    #[error("序列化失败")]
    Serialization,
    #[error("并发写入冲突")]
    ConcurrentWrite,
    #[error("事务损坏")]
    CorruptTransaction,
    #[error("I/O 失败")]
    Io,
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;

/// 记忆条目所在位置（原摆放于 ports，依赖方向回归 domain）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLocation {
    Active,
    Archive,
}

/// 淘汰候选值对象（原摆放于 ports，依赖方向回归 domain）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionCandidate {
    pub entry: MemoryEntry,
    pub ttl_expired: bool,
    pub eviction_score: i64,
    pub eviction_reason: String,
}

/// 搜索命中值对象（原摆放于 ports，依赖方向回归 domain）。
#[derive(Debug, Clone, PartialEq)]
pub struct MemorySearchHit {
    pub entry: MemoryEntry,
    pub location: MemoryLocation,
    pub outdated: bool,
    pub ttl_expired: bool,
    /// Which entry replaced this one (#1774). `search` keeps reporting
    /// superseded entries; only injection filters them out (M10).
    pub superseded_by: Option<MemoryId>,
    pub relevance: Option<f64>,
}

/// 显式搜索查询值对象（原摆放于 ports，依赖方向回归 domain）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchQuery {
    pub text: String,
    pub limit: usize,
    pub layer: Option<MemoryLayer>,
    pub category: Option<MemoryCategory>,
    pub include_archive: bool,
    pub now: u64,
}
