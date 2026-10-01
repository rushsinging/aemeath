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
