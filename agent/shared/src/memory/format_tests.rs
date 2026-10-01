use super::*;
use crate::memory::{MemoryEntry, MemorySource};

#[test]
fn test_parse_layer_valid() {
    assert_eq!(parse_layer("global"), Some(MemoryLayer::Global));
    assert_eq!(parse_layer("p"), Some(MemoryLayer::Project));
}

#[test]
fn test_parse_layer_invalid() {
    assert_eq!(parse_layer("session"), None);
    assert_eq!(parse_layer(""), None);
}

#[test]
fn test_parse_category_valid() {
    assert_eq!(parse_category("decision"), Some(MemoryCategory::Decision));
    assert_eq!(parse_category("pitfall"), Some(MemoryCategory::Pitfall));
}

#[test]
fn test_parse_category_invalid() {
    assert_eq!(parse_category("unknown"), None);
    assert_eq!(parse_category(""), None);
}

#[test]
fn test_format_memory_list_empty() {
    assert_eq!(format_memory_list(&[]), "暂无记忆。");
}

#[test]
fn test_format_memory_list_with_entry() {
    let entry = MemoryEntry::new(
        "memory-1",
        100,
        MemoryLayer::Project,
        MemoryCategory::Decision,
        "使用 JSON 存储",
        MemorySource::User,
    );
    let output = format_memory_list(&[entry]);

    assert!(output.contains("使用 JSON 存储"));
    assert!(output.contains("Project"));
    assert!(output.contains("Decision"));
}
