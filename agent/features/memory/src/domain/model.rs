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
    pub accessed_at: u64,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub outdated: bool,
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
            accessed_at: now,
            access_count: 0,
            outdated: false,
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
mod tests {
    use super::*;
    use crate::domain::{
        eviction_candidates, injection_score, is_injection_eligible, jaccard_similarity,
        search_tie_break_score,
    };

    fn entry(_id: &str) -> MemoryEntry {
        MemoryEntry::new(
            MemoryId::now_v7(),
            1_000_000,
            MemoryLayer::Project,
            MemoryCategory::Pattern,
            "保持 Memory 契约单一",
            MemorySource::User,
        )
        .unwrap()
    }

    #[test]
    fn memory_id_rejects_blank_and_round_trips_as_string() {
        assert!(MemoryId::new("   ").is_err());
        let id = MemoryId::new("01890f3c-7c00-7000-8000-000000000001").unwrap();
        assert_eq!(id.to_string(), "01890f3c-7c00-7000-8000-000000000001");
    }

    #[test]
    fn entry_rejects_blank_content() {
        let result = MemoryEntry::new(
            MemoryId::now_v7(),
            10,
            MemoryLayer::Project,
            MemoryCategory::Fact,
            "  ",
            MemorySource::User,
        );
        assert!(matches!(result, Err(MemoryError::InvalidEntry { .. })));
    }

    #[test]
    fn ttl_expires_only_after_created_at_plus_ttl() {
        let mut memory = entry("memory-ttl");
        memory.ttl = Some(Duration::from_secs(10));
        assert!(!memory.is_ttl_expired(1_000_010));
        assert!(memory.is_ttl_expired(1_000_011));
    }

    #[test]
    fn injection_eligibility_rejects_outdated_and_expired_even_when_pinned() {
        let mut outdated = entry("outdated");
        outdated.pinned = true;
        outdated.outdated = true;
        assert!(!is_injection_eligible(&outdated, 1_000_000));

        let mut expired = entry("expired");
        expired.pinned = true;
        expired.ttl = Some(Duration::from_secs(1));
        assert!(!is_injection_eligible(&expired, 1_000_002));
    }

    #[test]
    fn eligible_pinned_entry_outranks_max_access_unpinned_entry() {
        let mut pinned = entry("pinned");
        pinned.pinned = true;
        let mut frequent = entry("frequent");
        frequent.access_count = u32::MAX;
        assert!(injection_score(&pinned, 1_000_000) > injection_score(&frequent, 1_000_000));
    }

    #[test]
    fn search_tie_break_accepts_ineligible_entries() {
        let mut archived_fact = entry("archived");
        archived_fact.outdated = true;
        archived_fact.ttl = Some(Duration::from_secs(1));
        assert!(search_tie_break_score(&archived_fact, 1_000_002) >= 0);
    }

    #[test]
    fn jaccard_similarity_is_case_insensitive_and_bounded() {
        assert_eq!(jaccard_similarity("Rust Memory", "rust memory"), 1.0);
        assert_eq!(jaccard_similarity("", ""), 0.0);
        assert!((0.0..=1.0).contains(&jaccard_similarity("rust memory", "rust port")));
    }

    #[test]
    fn eviction_candidates_never_include_pinned_entries() {
        let mut pinned = entry("pinned");
        pinned.pinned = true;
        let normal = entry("normal");
        let candidates = eviction_candidates(&[pinned, normal.clone()], 5, 1_000_000);
        assert_eq!(candidates, vec![normal]);
    }
}
