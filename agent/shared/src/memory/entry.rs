use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Memory storage layer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    Global,
    Project,
}

/// Memory entry category.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    Fact,
    Decision,
    Preference,
    Pattern,
    Pitfall,
}

/// Source that created the memory entry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    Llm,
    Hook,
    User,
}

/// Persistent memory entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEntry {
    pub id: String,
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
        id: impl Into<String>,
        now: u64,
        layer: MemoryLayer,
        category: MemoryCategory,
        content: impl Into<String>,
        source: MemorySource,
    ) -> Self {
        Self {
            id: id.into(),
            layer,
            category,
            content: content.into(),
            source,
            source_ref: None,
            tags: Vec::new(),
            pinned: false,
            ttl: None,
            created_at: now,
            accessed_at: now,
            access_count: 0,
            outdated: false,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_source_ref(mut self, source_ref: impl Into<String>) -> Self {
        self.source_ref = Some(source_ref.into());
        self
    }

    pub fn touch(&mut self, now: u64) {
        self.accessed_at = now;
        self.access_count = self.access_count.saturating_add(1);
    }

    pub fn is_ttl_expired(&self, now: u64) -> bool {
        self.ttl
            .map(|ttl| now > self.created_at.saturating_add(ttl.as_secs()))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_entry_new() {
        let entry = MemoryEntry::new(
            "memory-1",
            123,
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "使用 JSON 文件存储 memory",
            MemorySource::User,
        );

        assert_eq!(entry.id, "memory-1");
        assert_eq!(entry.created_at, 123);
        assert_eq!(entry.accessed_at, 123);
        assert_eq!(entry.layer, MemoryLayer::Project);
        assert_eq!(entry.category, MemoryCategory::Decision);
        assert_eq!(entry.content, "使用 JSON 文件存储 memory");
        assert_eq!(entry.source, MemorySource::User);
        assert!(!entry.pinned);
    }

    #[test]
    fn test_memory_entry_touch() {
        let mut entry = MemoryEntry::new(
            "memory-2",
            100,
            MemoryLayer::Global,
            MemoryCategory::Preference,
            "中文回复",
            MemorySource::Llm,
        );

        entry.touch(123);

        assert_eq!(entry.accessed_at, 123);
        assert_eq!(entry.access_count, 1);
    }

    #[test]
    fn test_memory_entry_ttl_expired() {
        let mut entry = MemoryEntry::new(
            "memory-3",
            100,
            MemoryLayer::Project,
            MemoryCategory::Fact,
            "临时事实",
            MemorySource::Hook,
        );
        entry.created_at = 100;
        entry.ttl = Some(Duration::from_secs(10));

        assert!(!entry.is_ttl_expired(109));
        assert!(entry.is_ttl_expired(111));
    }

    #[test]
    fn test_memory_entry_serde_lowercase() {
        let entry = MemoryEntry::new(
            "memory-4",
            100,
            MemoryLayer::Project,
            MemoryCategory::Pitfall,
            "避免 print_stdout",
            MemorySource::User,
        );
        let json = serde_json::to_string(&entry).unwrap();

        assert!(json.contains("\"layer\":\"project\""));
        assert!(json.contains("\"category\":\"pitfall\""));
        assert!(json.contains("\"source\":\"user\""));
    }
}
